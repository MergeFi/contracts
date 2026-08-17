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
use soroban_sdk::{contract, contractimpl, token, Address, Env, Vec};
use types::{Contribution, DataKey, Escrow, EscrowStatus};

/// Basis points denominator (100.00%).
pub const BPS_DENOMINATOR: i128 = 10_000;

/// Maximum number of distinct contributions (sponsors) a single escrow can
/// accumulate. Bounds the per-contributor loops in `refund` and
/// `extend_deadline` to a small, predictable constant regardless of how
/// popular a bounty gets. See `docs/escrow-crowdfunding-design.md`.
pub const MAX_SPONSORS: u32 = 20;

/// Minimum grace period (in seconds) after the deadline before anyone can permissionlessly trigger a refund.
/// This prevents a race condition where a legitimate release in-flight near the deadline gets front-run by a refund.
pub const GRACE_PERIOD: u64 = 14 * 24 * 60 * 60; // 14 days

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    /// Atomic deployment-time setup. `admin` is the mergefi-backend oracle address that is
    /// authorized to call `release`/`refund` early; `treasury` receives the
    /// protocol fee; `fee_bps` is the fee charged on every payout, expressed
    /// in basis points (1/100th of a percent), e.g. 250 = 2.5%.
    ///
    /// The host invokes this constructor in the contract-creation operation,
    /// so no callable, uninitialized instance can exist. The named admin must
    /// also authorize the deployment.
    pub fn __constructor(
        env: Env,
        admin: Address,
        treasury: Address,
        fee_bps: u32,
    ) -> Result<(), Error> {
        admin.require_auth();

        if fee_bps as i128 > BPS_DENOMINATOR {
            return Err(Error::InvalidFee);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
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
    ) -> Result<(), Error> {
        sponsor.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
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

        if escrow.contributor_count >= MAX_SPONSORS {
            return Err(Error::TooManySponsors);
        }

        let token_client = token::Client::new(&env, &escrow.token);
        token_client.transfer(&sponsor, env.current_contract_address(), &amount);

        let contribution_key = DataKey::Contribution(issue_id, escrow.contributor_count);
        env.storage()
            .persistent()
            .set(&contribution_key, &Contribution { sponsor, amount });
        extend_ttl(&env, &contribution_key);

        escrow.amount += amount;
        escrow.contributor_count += 1;
        env.storage().persistent().set(&key, &escrow);
        extend_ttl(&env, &key);

        Ok(())
    }

    /// Releases escrowed funds to one or more recipients. `recipients` is a
    /// list of (address, basis_points) pairs that must sum to exactly
    /// `BPS_DENOMINATOR` (10000 = 100%). A protocol fee (`fee_bps`,
    /// configured at deployment) is deducted from the total and sent to
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

        let payouts = compute_split(&env, escrow.amount, &recipients)?;
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
            let contribution: Contribution =
                env.storage().persistent().get(&contribution_key).unwrap();
            token_client.transfer(
                &contract_address,
                &contribution.sponsor,
                &contribution.amount,
            );
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
        extend_ttl(&env, &key);

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
            .ok_or(Error::EscrowNotFound)
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
}

pub(crate) struct Payouts {
    pub fee: i128,
    pub shares: soroban_sdk::Vec<(Address, i128)>,
}

/// Validates that basis-point splits sum to exactly 10000 and computes the
/// treasury fee plus each recipient's absolute payout amount. Reused by the
/// milestone and maintenance-pool contracts conceptually (each keeps its own
/// copy today; see README "Why separate contracts" for the tradeoff).
pub(crate) fn compute_split(
    env: &Env,
    total: i128,
    recipients: &Vec<(Address, u32)>,
) -> Result<Payouts, Error> {
    if recipients.is_empty() {
        return Err(Error::InvalidSplit);
    }

    let mut bps_sum: i128 = 0;
    for (_, bps) in recipients.iter() {
        bps_sum += bps as i128;
    }
    if bps_sum != BPS_DENOMINATOR {
        return Err(Error::InvalidSplit);
    }

    let fee_bps: u32 = env
        .storage()
        .instance()
        .get(&DataKey::FeeBps)
        .ok_or(Error::NotInitialized)?;

    let fee = total * (fee_bps as i128) / BPS_DENOMINATOR;
    let distributable = total - fee;

    let mut shares: Vec<(Address, i128)> = Vec::new(env);
    let mut remainders: Vec<i128> = Vec::new(env);
    let mut allocated: i128 = 0;

    for (recipient, bps) in recipients.iter() {
        let numerator = distributable * (bps as i128);
        let share = numerator / BPS_DENOMINATOR;
        let remainder = numerator % BPS_DENOMINATOR;
        allocated += share;
        shares.push_back((recipient, share));
        remainders.push_back(remainder);
    }

    let mut dust = distributable - allocated;
    while dust > 0 {
        let mut best_index: u32 = 0;
        let mut best_remainder: i128 = -1;
        for (i, remainder) in remainders.iter().enumerate() {
            if remainder > best_remainder {
                best_index = i as u32;
                best_remainder = remainder;
            } else if remainder == best_remainder && remainder != -1 {
                let current_addr = shares.get(i as u32).unwrap().0;
                let best_addr = shares.get(best_index).unwrap().0;
                if current_addr < best_addr {
                    best_index = i as u32;
                    best_remainder = remainder;
                }
            }
        }

        let (recipient, share) = shares.get(best_index).unwrap();
        shares.set(best_index, (recipient, share + 1));
        remainders.set(best_index, -1);
        dust -= 1;
    }

    Ok(Payouts { fee, shares })
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
