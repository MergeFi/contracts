//! MergeFi Escrow Contract
//!
//! Holds sponsor-funded bounty escrows for individual GitHub issues and
//! releases them (in full or split across a team) once the mergefi-backend
//! oracle reports that the underlying work has been merged/accepted, or
//! refunds them back to the sponsor if the issue is cancelled / its deadline
//! passes unresolved.
#![no_std]

mod error;
mod types;

#[cfg(test)]
mod test;

use error::Error;
use mergefi_common::{BPS_DENOMINATOR, MAX_SPONSORS};
use soroban_sdk::{contract, contractimpl, token, Address, Env, Vec};
use types::{Contribution, DataKey, Escrow, EscrowStatus};

/// Minimum grace period (in seconds) after the deadline before anyone can permissionlessly trigger a refund.
/// This prevents a race condition where a legitimate release in-flight near the deadline gets front-run by a refund.
pub const GRACE_PERIOD: u64 = 14 * 24 * 60 * 60; // 14 days

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    /// One-time setup. `admin` is the mergefi-backend oracle address that is
    /// authorized to call `release`/`refund` early; `treasury` receives the
    /// protocol fee; `fee_bps` is the fee charged on every payout, expressed
    /// in basis points (1/100th of a percent), e.g. 250 = 2.5%.
    ///
    /// Requires `admin`'s own authorization, so nobody can name a
    /// third-party address as admin without that address's consent. This
    /// does *not* prevent an attacker from front-running the legitimate
    /// deployer's `initialize` call by naming themselves as admin instead
    /// — closing that race requires an atomic deploy+init (a Soroban
    /// constructor) rather than an in-contract check; see
    /// `docs/access-control-audit.md`.
    ///
    /// `max_sponsors` caps how many distinct contributions a single escrow
    /// may accumulate (see `contribute`); pass `None` to use the default
    /// `MAX_SPONSORS` (20).
    pub fn initialize(
        env: Env,
        admin: Address,
        treasury: Address,
        fee_bps: u32,
        max_sponsors: Option<u32>,
    ) -> Result<(), Error> {
        admin.require_auth();

        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        if fee_bps as i128 > BPS_DENOMINATOR {
            return Err(Error::InvalidFee);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
        env.storage().instance().set(
            &DataKey::MaxSponsors,
            &max_sponsors.unwrap_or(MAX_SPONSORS),
        );
        Ok(())
    }

    /// Sponsor deposits `amount` of `token` into escrow for `issue_id`,
    /// creating it. Requires the sponsor's authorization. `deadline` is a
    /// unix timestamp (ledger time) after which, if unpaid, contributors
    /// may reclaim their funds. One escrow per `issue_id` — a second `fund`
    /// call on the same id is rejected (`AlreadyFunded`); every sponsor
    /// after the first uses `contribute` instead. See
    /// `docs/escrow-crowdfunding-design.md` for why creation and
    /// contribution are kept as two separate entrypoints.
    ///
    /// `target` (issue #144) is an optional, informational funding goal —
    /// `Some(n)` requires `n > 0` (`InvalidTarget` otherwise), `None` means
    /// no goal is tracked (the pre-#144 behavior). Purely a UI hint: it is
    /// never checked against `amount`/`contribute` totals and never affects
    /// `release`/`refund`, so a bounty can be funded past it, released, or
    /// refunded regardless of whether the target was ever reached.
    ///
    /// Note: this contract has no visibility into `mergefi-milestones` —
    /// nothing here stops the same `issue_id` from also being allocated a
    /// budget via `milestones::allocate` for some release milestone. See
    /// README "Why three contracts instead of one" → "Cross-contract
    /// double-funding" for why that gap is accepted here and handled by
    /// `mergefi-backend` instead.
    pub fn fund(
        env: Env,
        issue_id: u64,
        sponsor: Address,
        token: Address,
        amount: i128,
        deadline: u64,
        target: Option<i128>,
    ) -> Result<(), Error> {
        sponsor.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if let Some(t) = target {
            if t <= 0 {
                return Err(Error::InvalidTarget);
            }
        }

        let key = DataKey::Escrow(issue_id);
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyFunded);
        }

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&sponsor, env.current_contract_address(), &amount);

        let contribution_key = DataKey::Contribution(issue_id, 0);
        env.storage()
            .persistent()
            .set(&contribution_key, &Contribution { sponsor, amount });
        extend_ttl(&env, &contribution_key);

        let escrow = Escrow {
            token,
            amount,
            status: EscrowStatus::Funded,
            created_at: env.ledger().timestamp(),
            deadline,
            contributor_count: 1,
            target,
        };
        env.storage().persistent().set(&key, &escrow);
        extend_ttl(&env, &key);

        Ok(())
    }

    /// Adds an additional sponsor's contribution to an already-funded
    /// escrow, enabling crowdfunding: several sponsors can co-fund the same
    /// `issue_id`. Requires the contributing sponsor's authorization. Uses
    /// the token already recorded on the escrow (no `token` parameter), so
    /// a top-up can never silently use a different asset than the original
    /// funder intended. Rejects `EscrowNotFound`, `AlreadyPaid`,
    /// `AlreadyRefunded`, and `TooManySponsors` once `MAX_SPONSORS`
    /// contributions have already been recorded.
    pub fn contribute(
        env: Env,
        issue_id: u64,
        sponsor: Address,
        amount: i128,
    ) -> Result<(), Error> {
        sponsor.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let key = DataKey::Escrow(issue_id);
        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::EscrowNotFound)?;

        match escrow.status {
            EscrowStatus::Paid => return Err(Error::AlreadyPaid),
            EscrowStatus::Refunded => return Err(Error::AlreadyRefunded),
            EscrowStatus::Funded => {}
        }

        let max_sponsors: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MaxSponsors)
            .unwrap_or(MAX_SPONSORS);
        let mut existing_index = None;
        for i in 0..escrow.contributor_count {
            let contribution_key = DataKey::Contribution(issue_id, i);
            let contribution: Contribution =
                env.storage().persistent().get(&contribution_key).unwrap();
            if contribution.sponsor == sponsor {
                existing_index = Some(i);
                break;
            }
        }

        if existing_index.is_none() && escrow.contributor_count >= max_sponsors {
            return Err(Error::TooManySponsors);
        }

        let token_client = token::Client::new(&env, &escrow.token);
        token_client.transfer(&sponsor, env.current_contract_address(), &amount);

        if let Some(index) = existing_index {
            let contribution_key = DataKey::Contribution(issue_id, index);
            let mut contribution: Contribution =
                env.storage().persistent().get(&contribution_key).unwrap();
            contribution.amount += amount;
            env.storage()
                .persistent()
                .set(&contribution_key, &contribution);
            extend_ttl(&env, &contribution_key);
        } else {
            let contribution_key = DataKey::Contribution(issue_id, escrow.contributor_count);
            env.storage()
                .persistent()
                .set(&contribution_key, &Contribution { sponsor, amount });
            extend_ttl(&env, &contribution_key);
            escrow.contributor_count += 1;
        }

        escrow.amount += amount;
        env.storage().persistent().set(&key, &escrow);
        extend_ttl(&env, &key);

        Ok(())
    }

    /// Releases escrowed funds to one or more recipients. `recipients` is a
    /// list of (address, basis_points) pairs that must sum to exactly
    /// `BPS_DENOMINATOR` (10000 = 100%). A protocol fee (`fee_bps`,
    /// configured at `initialize`) is deducted from the total and sent to
    /// the treasury; the remainder is split across recipients pro-rata.
    ///
    /// Only the admin (mergefi-backend oracle) may call this.
    pub fn release(env: Env, issue_id: u64, recipients: Vec<(Address, u32)>) -> Result<(), Error> {
        let admin = require_admin(&env)?;
        admin.require_auth();

        let key = DataKey::Escrow(issue_id);
        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::EscrowNotFound)?;

        match escrow.status {
            EscrowStatus::Paid => return Err(Error::AlreadyPaid),
            EscrowStatus::Refunded => return Err(Error::AlreadyRefunded),
            EscrowStatus::Funded => {}
        }

        let fee_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::FeeBps)
            .ok_or(Error::NotInitialized)?;
        let payouts = mergefi_common::compute_split(&env, escrow.amount, fee_bps, &recipients)
            .map_err(|_| Error::InvalidSplit)?;
        let treasury: Address = env.storage().instance().get(&DataKey::Treasury).unwrap();
        let token_client = token::Client::new(&env, &escrow.token);
        let contract_address = env.current_contract_address();

        if payouts.fee > 0 {
            token_client.transfer(&contract_address, &treasury, &payouts.fee);
        }
        for (recipient, share) in payouts.shares.iter() {
            if share > 0 {
                token_client.transfer(&contract_address, &recipient, &share);
            }
        }

        escrow.status = EscrowStatus::Paid;
        env.storage().persistent().set(&key, &escrow);
        extend_ttl(&env, &key);

        // Keep contribution sub-records alive alongside the parent so the
        // full ledger remains queryable after a release event.
        for i in 0..escrow.contributor_count {
            extend_ttl(&env, &DataKey::Contribution(issue_id, i));
        }

        Ok(())
    }

    /// Refunds every contributor their own contributed amount, to their own
    /// address — not just the full escrowed amount to a single sponsor.
    /// Callable by the admin at any time (e.g. issue cancelled), or by
    /// anyone once the escrow's deadline has passed. Because each
    /// contribution is stored as an exact amount rather than a share, no
    /// proportional-split math is needed: the sum refunded is exactly the
    /// sum contributed, returned along the same lines it arrived in. See
    /// `docs/escrow-crowdfunding-design.md`.
    pub fn refund(env: Env, issue_id: u64) -> Result<(), Error> {
        let key = DataKey::Escrow(issue_id);
        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::EscrowNotFound)?;

        match escrow.status {
            EscrowStatus::Paid => return Err(Error::AlreadyPaid),
            EscrowStatus::Refunded => return Err(Error::AlreadyRefunded),
            EscrowStatus::Funded => {}
        }

        let now = env.ledger().timestamp();
        if now < escrow.deadline + GRACE_PERIOD {
            // Not yet expired + grace period: only the admin may force an early refund.
            let admin = require_admin(&env)?;
            admin.require_auth();
        }

        let token_client = token::Client::new(&env, &escrow.token);
        let contract_address = env.current_contract_address();
        for i in 0..escrow.contributor_count {
            let contribution_key = DataKey::Contribution(issue_id, i);
            let contribution: Contribution = env
                .storage()
                .persistent()
                .get(&contribution_key)
                .ok_or(Error::ContributionNotFound)?;
            token_client.transfer(
                &contract_address,
                &contribution.sponsor,
                &contribution.amount,
            );
            // Refresh the contribution's TTL so the record stays readable
            // as a historical receipt after the refund event.
            extend_ttl(&env, &contribution_key);
        }

        escrow.status = EscrowStatus::Refunded;
        env.storage().persistent().set(&key, &escrow);
        extend_ttl(&env, &key);

        Ok(())
    }

    /// Pushes `issue_id`'s deadline further into the future. Callable by
    /// `caller`, who must be *any* current contributor to this escrow (not
    /// necessarily the original `fund` caller) — extending only ever
    /// delays `refund`'s permissionless path, never redirects funds or
    /// changes anyone's share, so it doesn't require unanimous or
    /// contribution-weighted consent from every contributor. See
    /// `docs/escrow-crowdfunding-design.md` for the full reasoning and
    /// `docs/refund-permissionless-analysis.md` for the original
    /// single-sponsor analysis this generalizes. `new_deadline` must be
    /// strictly later than both the current stored deadline and the
    /// current ledger time, so this can only ever delay the permissionless
    /// window, never shorten it.
    ///
    /// # What setting a far-future `new_deadline` does and does not guarantee
    ///
    /// The record's persistent-storage TTL is extended to approximately
    /// cover `new_deadline` (plus `GRACE_PERIOD`, so the permissionless
    /// `refund` window itself stays reachable), not just the flat ~29-day
    /// bump every other call in this contract applies — a `new_deadline`
    /// six months out genuinely buys roughly six months of survivability,
    /// not 29 days of it (MergeFi/contracts#56).
    ///
    /// That scaling is still capped at Soroban's actual persistent-entry TTL
    /// ceiling (`env.ledger().max_live_until_ledger()`, ~1 year on a
    /// typically-configured network) — nothing can extend a single entry's
    /// TTL past what the network itself allows in one call. A `new_deadline`
    /// beyond that ceiling still only receives the maximum extension this
    /// call can grant; the record is **not** guaranteed to survive all the
    /// way to a multi-year `new_deadline` from this call alone. For that,
    /// call `keep_alive` again periodically (at least once within the
    /// ceiling's own window) — it re-applies this same scaling without
    /// touching `deadline` or `status`, and needs no contributor
    /// authorization since it can only ever help the record survive longer,
    /// never change what it means.
    pub fn extend_deadline(
        env: Env,
        issue_id: u64,
        caller: Address,
        new_deadline: u64,
    ) -> Result<(), Error> {
        caller.require_auth();

        let key = DataKey::Escrow(issue_id);
        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::EscrowNotFound)?;

        match escrow.status {
            EscrowStatus::Paid => return Err(Error::AlreadyPaid),
            EscrowStatus::Refunded => return Err(Error::AlreadyRefunded),
            EscrowStatus::Funded => {}
        }

        let mut is_contributor = false;
        for i in 0..escrow.contributor_count {
            let contribution_key = DataKey::Contribution(issue_id, i);
            let contribution: Contribution =
                env.storage().persistent().get(&contribution_key).unwrap();
            if contribution.sponsor == caller {
                is_contributor = true;
                break;
            }
        }
        if !is_contributor {
            return Err(Error::Unauthorized);
        }

        if new_deadline <= escrow.deadline || new_deadline <= env.ledger().timestamp() {
            return Err(Error::InvalidDeadline);
        }

        escrow.deadline = new_deadline;
        env.storage().persistent().set(&key, &escrow);
        extend_ttl_for_target(&env, &key, new_deadline.saturating_add(GRACE_PERIOD));

        // Extend every contribution sub-record to the same target so they
        // can't archive ahead of the parent record when the deadline is
        // pushed far into the future.
        let target = new_deadline.saturating_add(GRACE_PERIOD);
        for i in 0..escrow.contributor_count {
            extend_ttl_for_target(&env, &DataKey::Contribution(issue_id, i), target);
        }

        Ok(())
    }

    /// Permissionless TTL refresh: re-extends `issue_id`'s persistent-storage
    /// TTL toward its currently-stored `deadline` (plus `GRACE_PERIOD`),
    /// without touching `deadline` or `status`. Exists because a single
    /// `extend_deadline` call can only extend TTL up to Soroban's own
    /// persistent-entry ceiling (`env.ledger().max_live_until_ledger()`, not
    /// unlimited) — a `deadline` set beyond that ceiling needs this called
    /// again periodically (by the sponsor, any contributor, or an automated
    /// `mergefi-backend` job) to keep surviving toward it, since no single
    /// call can cover unlimited future time (#56).
    ///
    /// Also refreshes every `Contribution(issue_id, i)` sub-record toward the
    /// same target — individual contribution entries have their own TTL and
    /// will archive independently of the parent `Escrow` record if never
    /// re-extended, silently breaking `refund` for long-lived escrows where
    /// older contributions have fallen off-ledger while the parent stayed
    /// alive via prior `keep_alive` / `extend_deadline` calls.
    ///
    /// Callable by anyone and needs no authorization: it can only ever keep
    /// records alive longer, never change what they hold or who they pay.
    pub fn keep_alive(env: Env, issue_id: u64) -> Result<(), Error> {
        let key = DataKey::Escrow(issue_id);
        let escrow: Escrow = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::EscrowNotFound)?;

        let target = escrow.deadline.saturating_add(GRACE_PERIOD);
        extend_ttl_for_target(&env, &key, target);

        // Keep every contribution sub-record alive toward the same target so
        // they can't archive independently while the parent Escrow lives on.
        for i in 0..escrow.contributor_count {
            let contribution_key = DataKey::Contribution(issue_id, i);
            extend_ttl_for_target(&env, &contribution_key, target);
        }

        Ok(())
    }

    /// Returns the escrow record for `issue_id`.
    pub fn get_escrow(env: Env, issue_id: u64) -> Result<Escrow, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Escrow(issue_id))
            .ok_or(Error::EscrowNotFound)
    }

    /// Returns the `index`-th contribution recorded for `issue_id` (`0` is
    /// always the original `fund` caller; subsequent indices are
    /// `contribute` calls in the order they were accepted), letting
    /// off-chain callers enumerate the full contribution ledger for an
    /// escrow via `0..escrow.contributor_count`.
    pub fn get_contribution(env: Env, issue_id: u64, index: u32) -> Result<Contribution, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Contribution(issue_id, index))
            .ok_or(Error::ContributionNotFound)
    }

    /// Returns every contribution recorded for `issue_id` in one call
    /// (issue #145) — `0..escrow.contributor_count`, in the same order
    /// `get_contribution` exposes individually. Bounded by `MAX_SPONSORS`
    /// (20), so this is always a small, cheap read: replaces up to 20
    /// separate simulated RPC calls (one per `get_contribution` index) with
    /// one for the common "show me who funded this bounty" UI case.
    pub fn get_contributions(env: Env, issue_id: u64) -> Result<Vec<Contribution>, Error> {
        let escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(issue_id))
            .ok_or(Error::EscrowNotFound)?;

        let mut contributions = Vec::new(&env);
        for i in 0..escrow.contributor_count {
            let contribution: Contribution = env
                .storage()
                .persistent()
                .get(&DataKey::Contribution(issue_id, i))
                .ok_or(Error::ContributionNotFound)?;
            contributions.push_back(contribution);
        }
        Ok(contributions)
    }

    pub fn get_admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_treasury(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Treasury)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_fee_bps(env: Env) -> Result<u32, Error> {
        env.storage()
            .instance()
            .get(&DataKey::FeeBps)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_max_sponsors(env: Env) -> Result<u32, Error> {
        env.storage()
            .instance()
            .get(&DataKey::MaxSponsors)
            .ok_or(Error::NotInitialized)
    }
}

pub(crate) fn require_admin(env: &Env) -> Result<Address, Error> {
    mergefi_common::require_admin::<DataKey>(env).ok_or(Error::NotInitialized)
}

/// Extends the TTL of a persistent entry so escrow records aren't archived
/// while still active. Threshold/extend values are conservative defaults
/// suitable for a multi-month bounty lifecycle.
pub(crate) fn extend_ttl(env: &Env, key: &DataKey) {
    mergefi_common::extend_ttl(env, key);
}

/// Extends the TTL of a persistent entry to (approximately) survive until
/// `target_timestamp`, capped at Soroban's own persistent-entry TTL
/// ceiling — see `mergefi_common::extend_ttl_for_target` for the full
/// derivation and rationale (#56).
pub(crate) fn extend_ttl_for_target(env: &Env, key: &DataKey, target_timestamp: u64) {
    mergefi_common::extend_ttl_for_target(env, key, target_timestamp);
}
