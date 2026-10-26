//! MergeFi Milestone Funding Contract
//!
//! A milestone pools one or more sponsors' contributions into a lump-sum
//! budget shared across multiple GitHub issues that make up a release.
//! The first sponsor deposits to open the pool; the backend oracle
//! allocates slices of the budget to individual issues and later releases
//! each allocation (optionally split across a team) as issues are merged,
//! exactly like the escrow contract's `release`, but drawn from a shared
//! pool instead of a single-issue deposit. If the release is cancelled,
//! the unallocated remainder is refunded to every contributor in
//! proportion to what they put in.
#![no_std]
#![warn(missing_docs)]

mod error;
mod types;

#[cfg(test)]
mod test;

use error::Error;
use soroban_sdk::{contract, contractevent, contractimpl, token, Address, BytesN, Env, Map, Vec};
use types::{Contribution, DataKey, IssueStatus, Milestone};

use mergefi_common::{
    extend_instance_ttl, BPS_DENOMINATOR, MAX_SPONSORS, MAX_SPONSORS_LIMIT,
};

/// Minimum grace period (in seconds) after the deadline before anyone can
/// permissionlessly trigger a cancel_milestone. Mirrors escrow's
/// GRACE_PERIOD — prevents a race where a legitimate release_issue in
/// flight near the deadline gets front-run by a permissionless cancel.
pub const GRACE_PERIOD: u64 = 14 * 24 * 60 * 60; // 14 days

/// Current version of the storage schema. Incremented on breaking layout changes.
const CONTRACT_VERSION: u32 = 2;

// The `#[contract]` and `#[contractimpl]` macros generate additional items
// (instance storage fields, spec functions, client methods, arg helpers)
// that rustdoc sees but that aren't annotated here. Suppress the lint at the
// module level for that macro-generated surface, then re-enable it on the
// hand-written `#[contractimpl]` block below so every entrypoint is still
// checked.
#[allow(missing_docs)]
mod contract {
    use super::*;

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct ContractInitialized {
        #[topic]
        pub admin: Address,
        #[topic]
        pub oracle: Address,
        pub treasury: Address,
        pub fee_bps: u32,
        pub max_sponsors: u32,
        pub recovery: Option<Address>,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct MilestoneCreated {
        #[topic]
        pub milestone_id: u64,
        #[topic]
        pub sponsor: Address,
        pub token: Address,
        pub amount: i128,
        pub deadline: u64,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct MilestoneContributed {
        #[topic]
        pub milestone_id: u64,
        #[topic]
        pub sponsor: Address,
        pub amount: i128,
        pub total_budget: i128,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct MilestoneAllocated {
        #[topic]
        pub milestone_id: u64,
        #[topic]
        pub issue_id: u64,
        pub amount: i128,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct MilestoneDeallocated {
        #[topic]
        pub milestone_id: u64,
        #[topic]
        pub issue_id: u64,
        pub amount: i128,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct IssueReleased {
        #[topic]
        pub milestone_id: u64,
        #[topic]
        pub issue_id: u64,
        pub amount: i128,
        pub fee: i128,
        pub treasury: Address,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct IssuePaidTo {
        #[topic]
        pub milestone_id: u64,
        #[topic]
        pub issue_id: u64,
        pub recipient: Address,
        pub amount: i128,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct MilestoneCancelled {
        #[topic]
        pub milestone_id: u64,
        pub refunded_amount: i128,
        pub permissionless: bool,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct MilestoneRefundPaid {
        #[topic]
        pub milestone_id: u64,
        #[topic]
        pub sponsor: Address,
        pub amount: i128,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct MilestoneKeptAlive {
        #[topic]
        pub milestone_id: u64,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct ContractPaused {
        #[topic]
        pub admin: Address,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct ContractUnpaused {
        #[topic]
        pub admin: Address,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct AdminChanged {
        #[topic]
        pub old_admin: Address,
        #[topic]
        pub new_admin: Address,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct AdminRecovered {
        #[topic]
        pub recovery: Address,
        #[topic]
        pub new_admin: Address,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct OracleChanged {
        #[topic]
        pub old_oracle: Address,
        #[topic]
        pub new_oracle: Address,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct TreasuryChanged {
        #[topic]
        pub old_treasury: Address,
        #[topic]
        pub new_treasury: Address,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct FeeBpsChanged {
        pub old_fee_bps: u32,
        pub new_fee_bps: u32,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct ContractUpgraded {
        #[topic]
        pub admin: Address,
        pub wasm_hash: BytesN<32>,
        pub version: u32,
    }

    #[contract]
    pub struct MilestonesContract;

    #[warn(missing_docs)]
    #[contractimpl]
    impl MilestonesContract {
        /// One-time setup. Requires `admin`'s own authorization, so nobody can
        /// name a third-party address as admin without that address's consent
        /// — see `docs/access-control-audit.md` for what this does and does
        /// not protect against (it does not stop initializer front-running).
        ///
        /// `max_sponsors` caps how many distinct contributions a single
        /// milestone may accumulate (see `contribute`); pass `None` to use the
        /// default `MAX_SPONSORS` (20). Custom values may not exceed
        /// `MAX_SPONSORS_LIMIT` (50), keeping contributor loops bounded.
        pub fn initialize(
            env: Env,
            admin: Address,
            oracle: Address,
            treasury: Address,
            fee_bps: u32,
            max_sponsors: Option<u32>,
            recovery: Option<Address>,
        ) -> Result<(), Error> {
            admin.require_auth();
            oracle.require_auth();

            if env.storage().instance().has(&DataKey::Admin) {
                return Err(Error::AlreadyInitialized);
            }
            if fee_bps as i128 > BPS_DENOMINATOR {
                return Err(Error::InvalidFee);
            }
            if treasury == env.current_contract_address() {
                return Err(Error::InvalidTreasury);
            }
            let max_sponsors = max_sponsors.unwrap_or(MAX_SPONSORS);
            if max_sponsors > MAX_SPONSORS_LIMIT {
                return Err(Error::InvalidMaxSponsors);
            }
            env.storage().instance().set(&DataKey::Admin, &admin);
            env.storage().instance().set(&DataKey::Oracle, &oracle);
            env.storage().instance().set(&DataKey::Treasury, &treasury);
            env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
            env.storage()
                .instance()
                .set(&DataKey::MaxSponsors, &max_sponsors);
            env.storage()
                .instance()
                .set(&DataKey::Version, &CONTRACT_VERSION);
            env.storage().instance().set(&DataKey::Paused, &false);
            if let Some(r) = recovery.clone() {
                // Recovery address is explicitly set once at initialization and
                // cannot be changed later. It allows recovery of the admin key
                // if it is lost; see docs/access-control-audit.md for rationale.
                env.storage().instance().set(&DataKey::Recovery, &r);
            }
            extend_instance_ttl(&env);
            ContractInitialized {
                admin,
                oracle,
                treasury,
                fee_bps,
                max_sponsors,
                recovery,
            }
            .publish(&env);
            Ok(())
        }

        /// The original sponsor deposits `total_budget` of `token` to open a
        /// new milestone pool. Requires that sponsor's authorization. One
        /// milestone per `milestone_id` — a second `create_milestone` call on
        /// the same id is rejected (`IssueAlreadyAllocated`); every sponsor
        /// after the first uses `contribute` instead. See
        /// `docs/milestones-crowdfunding-design.md` for why creation and
        /// contribution are kept as two separate entrypoints.
        pub fn create_milestone(
            env: Env,
            milestone_id: u64,
            sponsor: Address,
            token: Address,
            total_budget: i128,
            deadline: u64,
        ) -> Result<(), Error> {
            if is_paused(&env) {
                return Err(Error::ContractPaused);
            }

            sponsor.require_auth();

            if total_budget <= 0 {
                return Err(Error::InvalidAmount);
            }

            // Reject a deadline that is already in the past: cancel_milestone_after_deadline
            // becomes callable by anyone as soon as deadline + GRACE_PERIOD elapses, so a
            // stale / zero deadline makes that permissionless path immediately reachable
            // (issue #491 — same gap as escrow's issue #21).
            if deadline <= env.ledger().timestamp() {
                return Err(Error::DeadlineInPast);
            }

            let key = DataKey::Milestone(milestone_id);
            if let Some(existing) = env.storage().persistent().get::<_, Milestone>(&key) {
                if !existing.closed {
                    return Err(Error::MilestoneAlreadyExists);
                }
                // Allow re-creation after terminal state (#41).
            }

            let token_client = token::Client::new(&env, &token);
            let actual_received = mergefi_common::measure_transfer_delta(
                &env,
                &token,
                &env.current_contract_address(),
                || {
                    token_client.transfer(&sponsor, env.current_contract_address(), &total_budget);
                },
            );

            if actual_received <= 0 {
                return Err(Error::InvalidAmount);
            }

            // The original funder is always contribution index 0, exactly like
            // `escrow::fund`; every later sponsor appends via `contribute`.
            let contribution_key = DataKey::Contribution(milestone_id, 0);
            env.storage().persistent().set(
                &contribution_key,
                &Contribution {
                    sponsor: sponsor.clone(),
                    amount: actual_received,
                    remaining: actual_received,
                    timestamp: env.ledger().timestamp(),
                },
            );
            extend_ttl(&env, &contribution_key);

            let milestone = Milestone {
                sponsor: sponsor.clone(),
                token: token.clone(),
                total_budget: actual_received,
                remaining_budget: actual_received,
                created_at: env.ledger().timestamp(),
                deadline,
                closed: false,
                allocations: Map::new(&env),
                contributor_count: 1,
            };
            env.storage().persistent().set(&key, &milestone);
            extend_ttl(&env, &key);
            extend_instance_ttl(&env);
            MilestoneCreated {
                milestone_id,
                sponsor,
                token,
                amount: actual_received,
                deadline,
            }
            .publish(&env);
            Ok(())
        }

        /// Adds an additional sponsor's contribution to an already-created
        /// milestone, enabling crowdfunding: several sponsors can co-fund the
        /// same `milestone_id`. Requires the contributing sponsor's
        /// authorization. Uses the token already recorded on the milestone (no
        /// `token` parameter), so a top-up can never silently use a different
        /// asset than the original funder intended. Rejects `MilestoneNotFound`,
        /// `MilestoneClosed`, `DeadlinePassed` once `now >= deadline` (issue #568),
        /// and `TooManySponsors` once `MAX_SPONSORS` contributions have already
        /// been recorded.
        pub fn contribute(
            env: Env,
            milestone_id: u64,
            sponsor: Address,
            amount: i128,
        ) -> Result<(), Error> {
            if is_paused(&env) {
                return Err(Error::ContractPaused);
            }

            sponsor.require_auth();

            if amount <= 0 {
                return Err(Error::InvalidAmount);
            }

            let mkey = DataKey::Milestone(milestone_id);
            let mut milestone: Milestone = env
                .storage()
                .persistent()
                .get(&mkey)
                .ok_or(Error::MilestoneNotFound)?;

            if milestone.closed {
                return Err(Error::MilestoneClosed);
            }

            if env.ledger().timestamp() >= milestone.deadline {
                return Err(Error::DeadlinePassed);
            }
            let max_sponsors: u32 = env
                .storage()
                .instance()
                .get(&DataKey::MaxSponsors)
                .unwrap_or(MAX_SPONSORS);
            let mut existing_index = None;
            for i in 0..milestone.contributor_count {
                let contribution_key = DataKey::Contribution(milestone_id, i);
                let contribution: Contribution = env
                    .storage()
                    .persistent()
                    .get(&contribution_key)
                    .ok_or(Error::ContributionNotFound)?;
                if contribution.sponsor == sponsor {
                    existing_index = Some(i);
                    break;
                }
            }

            if existing_index.is_none() && milestone.contributor_count >= max_sponsors {
                return Err(Error::TooManySponsors);
            }

            let token_client = token::Client::new(&env, &milestone.token);
            let actual_received = mergefi_common::measure_transfer_delta(
                &env,
                &milestone.token,
                &env.current_contract_address(),
                || {
                    token_client.transfer(&sponsor, env.current_contract_address(), &amount);
                },
            );

            if actual_received <= 0 {
                return Err(Error::InvalidAmount);
            }

            if let Some(index) = existing_index {
                let contribution_key = DataKey::Contribution(milestone_id, index);
                let mut contribution: Contribution = env
                    .storage()
                    .persistent()
                    .get(&contribution_key)
                    .ok_or(Error::ContributionNotFound)?;
                contribution.amount = contribution
                    .amount
                    .checked_add(actual_received)
                    .ok_or(Error::ArithmeticOverflow)?;
                contribution.remaining = contribution
                    .remaining
                    .checked_add(actual_received)
                    .ok_or(Error::ArithmeticOverflow)?;
                contribution.timestamp = env.ledger().timestamp();
                env.storage()
                    .persistent()
                    .set(&contribution_key, &contribution);
                extend_ttl(&env, &contribution_key);
            } else {
                let contribution_key =
                    DataKey::Contribution(milestone_id, milestone.contributor_count);
                env.storage().persistent().set(
                    &contribution_key,
                    &Contribution {
                        sponsor: sponsor.clone(),
                        amount: actual_received,
                        remaining: actual_received,
                        timestamp: env.ledger().timestamp(),
                    },
                );
                extend_ttl(&env, &contribution_key);
                milestone.contributor_count += 1;
            }

            // New funds arrive unallocated: the pool's total *and* its
            // unallocated remainder both grow by exactly the contribution, so
            // a later proportional refund treats them like any other share.
            milestone.total_budget = milestone
                .total_budget
                .checked_add(actual_received)
                .ok_or(Error::ArithmeticOverflow)?;
            milestone.remaining_budget = milestone
                .remaining_budget
                .checked_add(actual_received)
                .ok_or(Error::ArithmeticOverflow)?;
            env.storage().persistent().set(&mkey, &milestone);
            extend_ttl(&env, &mkey);

            // Refresh every prior contribution's TTL so older records don't
            // archive ahead of the parent while the milestone stays active.
            // The newly-written record (contributor_count - 1) was already
            // extended above; iterate the full range to cover all of them.
            for i in 0..milestone.contributor_count {
                extend_ttl(&env, &DataKey::Contribution(milestone_id, i));
            }
            extend_instance_ttl(&env);
            MilestoneContributed {
                milestone_id,
                sponsor,
                amount: actual_received,
                total_budget: milestone.total_budget,
            }
            .publish(&env);

            Ok(())
        }

        /// Admin-only: reserves `amount` of the milestone's remaining budget for
        /// `issue_id`. Rejects if the issue is already allocated, the milestone
        /// is closed, or `amount` exceeds the remaining (unallocated) budget.
        ///
        /// Note: this contract has no visibility into `mergefi-escrow` —
        /// nothing here stops the same `issue_id` from also being funded via
        /// `escrow::fund` as a standalone bounty. See README "Why three
        /// contracts instead of one" → "Cross-contract double-funding" for why
        /// that gap is accepted here and handled by `mergefi-backend` instead.
        pub fn allocate(
            env: Env,
            milestone_id: u64,
            issue_id: u64,
            amount: i128,
        ) -> Result<(), Error> {
            if is_paused(&env) {
                return Err(Error::ContractPaused);
            }

            require_admin(&env)?.require_auth();

            if amount <= 0 {
                return Err(Error::InvalidAmount);
            }

            let mkey = DataKey::Milestone(milestone_id);
            let mut milestone: Milestone = env
                .storage()
                .persistent()
                .get(&mkey)
                .ok_or(Error::MilestoneNotFound)?;

            if milestone.closed {
                return Err(Error::MilestoneClosed);
            }

            // Reject allocations once the milestone's deadline has passed (#489).
            // Once `now >= deadline` sponsors can trigger `cancel_milestone_after_deadline`
            // after the grace period; allowing allocations beyond the deadline lets the
            // admin pre-empt that cancel by moving remaining_budget into allocations that
            // the permissionless cancel cannot sweep.
            let now = env.ledger().timestamp();
            if now >= milestone.deadline {
                return Err(Error::DeadlinePassed);
            }

            if milestone.allocations.contains_key(issue_id) {
                return Err(Error::IssueAlreadyAllocated);
            }
            if amount > milestone.remaining_budget {
                return Err(Error::OverAllocation);
            }

            milestone.remaining_budget = milestone
                .remaining_budget
                .checked_sub(amount)
                .ok_or(Error::ArithmeticOverflow)?;
            milestone.allocations.set(issue_id, amount);
            env.storage().persistent().set(&mkey, &milestone);
            extend_ttl(&env, &mkey);

            // Refresh all contribution sub-records so they stay alive alongside
            // the parent record as the milestone accumulates allocations over time.
            for i in 0..milestone.contributor_count {
                extend_ttl(&env, &DataKey::Contribution(milestone_id, i));
            }

            let skey = DataKey::IssueStatus(milestone_id, issue_id);
            env.storage()
                .persistent()
                .set(&skey, &IssueStatus::Allocated);
            extend_ttl(&env, &skey);
            extend_instance_ttl(&env);
            MilestoneAllocated {
                milestone_id,
                issue_id,
                amount,
            }
            .publish(&env);

            Ok(())
        }

        /// Admin-only: releases the previously allocated amount for `issue_id`
        /// to `recipients` (basis points summing to 10000), minus the protocol
        /// fee, exactly as in the escrow contract.
        ///
        /// Issue #5 fix: This now checks if the milestone is closed and rejects
        /// the release with `Error::MilestoneClosed`. To release funds after
        /// deciding to cancel, use `deallocate` first to move allocated amounts
        /// back to `remaining_budget`, then call `cancel_milestone` to refund.
        pub fn release_issue(
            env: Env,
            milestone_id: u64,
            issue_id: u64,
            recipients: Vec<(Address, u32)>,
        ) -> Result<(), Error> {
            if is_paused(&env) {
                return Err(Error::ContractPaused);
            }

            require_oracle(&env)?.require_auth();

            let mkey = DataKey::Milestone(milestone_id);
            let milestone: Milestone = env
                .storage()
                .persistent()
                .get(&mkey)
                .ok_or(Error::MilestoneNotFound)?;

            // Issue #5 fix: Block release_issue on closed milestones
            if milestone.closed {
                return Err(Error::MilestoneClosed);
            }

            let skey = DataKey::IssueStatus(milestone_id, issue_id);
            let status: IssueStatus = env
                .storage()
                .persistent()
                .get(&skey)
                .ok_or(Error::IssueNotAllocated)?;
            if status == IssueStatus::Released {
                return Err(Error::IssueAlreadyReleased);
            }

            let amount = milestone
                .allocations
                .get(issue_id)
                .ok_or(Error::IssueNotAllocated)?;

            let fee_bps: u32 =
                mergefi_common::get_fee_bps::<DataKey>(&env).ok_or(Error::NotInitialized)?;
            let payouts = mergefi_common::compute_split(&env, amount, fee_bps, &recipients)
                .map_err(|e| match e {
                    mergefi_common::SplitError::InvalidSplit => Error::InvalidSplit,
                    mergefi_common::SplitError::SelfPayout => Error::SelfPayout,
                    mergefi_common::SplitError::TooManyRecipients => Error::TooManyRecipients,
                    mergefi_common::SplitError::Overflow => Error::ArithmeticOverflow,
                })?;
            let treasury: Address =
                mergefi_common::require_treasury::<DataKey>(&env).ok_or(Error::NotInitialized)?;
            let token_client = token::Client::new(&env, &milestone.token);
            let contract_address = env.current_contract_address();

            env.storage()
                .persistent()
                .set(&skey, &IssueStatus::Released);
            extend_ttl(&env, &skey);

            if payouts.fee > 0 {
                token_client.transfer(&contract_address, &treasury, &payouts.fee);
            }
            for (recipient, share) in payouts.shares.iter() {
                if share > 0 {
                    token_client.transfer(&contract_address, &recipient, &share);
                    IssuePaidTo {
                        milestone_id,
                        issue_id,
                        recipient,
                        amount: share,
                    }
                    .publish(&env);
                }
            }
            IssueReleased {
                milestone_id,
                issue_id,
                amount,
                fee: payouts.fee,
                treasury,
            }
            .publish(&env);

            // Refresh the parent milestone's Contribution sub-records so they
            // stay alive as individual issues are released over the milestone's
            // lifetime — release_issue doesn't touch contributions directly but
            // is called repeatedly on long-lived milestones.
            //
            // Deduct the release amount from contributions FIFO (first in, first out)
            // to track each sponsor's unspent remainder for accurate refunds.
            let mut to_deduct = amount;
            for i in 0..milestone.contributor_count {
                if to_deduct <= 0 {
                    break;
                }
                let contribution_key = DataKey::Contribution(milestone_id, i);
                let mut contribution: Contribution = env
                    .storage()
                    .persistent()
                    .get(&contribution_key)
                    .ok_or(Error::MilestoneNotFound)?;
                
                let deduction = to_deduct.min(contribution.remaining);
                contribution.remaining = contribution
                    .remaining
                    .checked_sub(deduction)
                    .ok_or(Error::ArithmeticOverflow)?;
                to_deduct = to_deduct
                    .checked_sub(deduction)
                    .ok_or(Error::ArithmeticOverflow)?;
                
                env.storage()
                    .persistent()
                    .set(&contribution_key, &contribution);
                extend_ttl(&env, &DataKey::Contribution(milestone_id, i));
            }
            extend_instance_ttl(&env);

            Ok(())
        }

        /// Admin-only: closes the milestone and refunds any unallocated budget
        /// back to its contributors, in proportion to what each one put in
        /// (e.g. release cancelled with issues remaining). A single-sponsor
        /// milestone is the degenerate case: the whole remainder goes back to
        /// the one sponsor, exactly as before this contract supported
        /// crowdfunding. See `refund_remaining_budget` and
        /// `docs/milestones-crowdfunding-design.md`.
        pub fn cancel_milestone(env: Env, milestone_id: u64) -> Result<(), Error> {
            require_admin(&env)?.require_auth();

            let mkey = DataKey::Milestone(milestone_id);
            let mut milestone: Milestone = env
                .storage()
                .persistent()
                .get(&mkey)
                .ok_or(Error::MilestoneNotFound)?;

            if milestone.closed {
                return Err(Error::MilestoneClosed);
            }

            let refunded_amount = milestone.remaining_budget;
            if milestone.remaining_budget > 0 {
                refund_remaining_budget(&env, milestone_id, &milestone)?;
                milestone.remaining_budget = 0;
            }
            milestone.closed = true;
            env.storage().persistent().set(&mkey, &milestone);
            extend_ttl(&env, &mkey);
            extend_instance_ttl(&env);
            MilestoneCancelled {
                milestone_id,
                refunded_amount,
                permissionless: false,
            }
            .publish(&env);
            Ok(())
        }

        /// Admin-only: deallocates a previously allocated (but not yet released)
        /// issue, moving its amount back into `remaining_budget`. This unblocks
        /// scenarios where an allocation was made in error or the issue is no
        /// longer relevant.
        ///
        /// **Required workflow for proper milestone cancellation (Issue #5):**
        /// 1. Call `deallocate` for each allocated-but-unreleased issue
        /// 2. Call `cancel_milestone` to refund the remaining_budget
        ///
        /// After Issue #5 fix, `release_issue` blocks on closed milestones, so
        /// allocated funds would be permanently stuck without deallocating first.
        ///
        /// Rejects if the issue is already Released (funds have left the
        /// contract) or not currently Allocated.
        ///
        /// Intentionally **not blocked** while paused, as it is a sponsor-protective
        /// operation that moves funds back toward refund paths (see
        /// docs/pause-circuit-breaker-design.md principle 1). During a pause,
        /// sponsors can use deallocate + cancel_milestone to recover allocated funds.
        pub fn deallocate(env: Env, milestone_id: u64, issue_id: u64) -> Result<(), Error> {
            require_admin(&env)?.require_auth();

            let mkey = DataKey::Milestone(milestone_id);
            let mut milestone: Milestone = env
                .storage()
                .persistent()
                .get(&mkey)
                .ok_or(Error::MilestoneNotFound)?;

            let skey = DataKey::IssueStatus(milestone_id, issue_id);
            let status: IssueStatus = env
                .storage()
                .persistent()
                .get(&skey)
                .ok_or(Error::IssueNotAllocatedForDeallocate)?;
            if status == IssueStatus::Released {
                return Err(Error::IssueAlreadyReleased);
            }

            let amount = milestone
                .allocations
                .get(issue_id)
                .ok_or(Error::IssueNotAllocatedForDeallocate)?;

            milestone.remaining_budget = milestone
                .remaining_budget
                .checked_add(amount)
                .ok_or(Error::ArithmeticOverflow)?;
            milestone.allocations.remove(issue_id);

            if milestone.closed && milestone.remaining_budget > 0 {
                refund_remaining_budget(&env, milestone_id, &milestone)?;
                milestone.remaining_budget = 0;
            }

            env.storage().persistent().set(&mkey, &milestone);
            extend_ttl(&env, &mkey);
            MilestoneDeallocated {
                milestone_id,
                issue_id,
                amount,
            }
            .publish(&env);
            Ok(())
        }

        /// Pause the contract, blocking new milestones, contributions,
        /// allocations, deallocations, and release payouts. Refund/recovery paths
        /// remain open.
        pub fn pause(env: Env) -> Result<(), Error> {
            let admin = require_admin(&env)?;
            admin.require_auth();

            env.storage().instance().set(&DataKey::Paused, &true);
            extend_instance_ttl(&env);
            ContractPaused { admin }.publish(&env);
            Ok(())
        }

        /// Unpause the contract, restoring normal operation.
        pub fn unpause(env: Env) -> Result<(), Error> {
            let admin = require_admin(&env)?;
            admin.require_auth();

            env.storage().instance().set(&DataKey::Paused, &false);
            extend_instance_ttl(&env);
            ContractUnpaused { admin }.publish(&env);
            Ok(())
        }

        /// Returns whether the contract is currently paused.
        pub fn is_paused_view(env: Env) -> bool {
            env.storage()
                .instance()
                .get(&DataKey::Paused)
                .unwrap_or(false)
        }

        /// Upgrade the contract's wasm code. Requires admin authorization.
        pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
            let admin = require_admin(&env)?;
            admin.require_auth();

            env.deployer()
                .update_current_contract_wasm(new_wasm_hash.clone());
            env.storage()
                .instance()
                .set(&DataKey::Version, &CONTRACT_VERSION);
            extend_instance_ttl(&env);
            ContractUpgraded {
                admin,
                wasm_hash: new_wasm_hash,
                version: CONTRACT_VERSION,
            }
            .publish(&env);
            Ok(())
        }

        /// Apply storage migrations through this code version. Call after
        /// `upgrade`; an already-migrated contract accepts a retry.
        pub fn migrate(env: Env, from_version: u32) -> Result<(), Error> {
            require_admin(&env)?.require_auth();

            let current_version = env
                .storage()
                .instance()
                .get::<_, u32>(&DataKey::Version)
                .unwrap_or(0);
            if current_version == CONTRACT_VERSION {
                extend_instance_ttl(&env);
                return Ok(());
            }
            if current_version != from_version || from_version >= CONTRACT_VERSION {
                return Err(Error::InvalidVersion);
            }

            env.storage()
                .instance()
                .set(&DataKey::Version, &CONTRACT_VERSION);
            extend_instance_ttl(&env);
            Ok(())
        }

        /// Permissionless cancel after the milestone's deadline has passed
        /// (plus a grace period). Mirrors escrow's permissionless `refund`:
        /// anyone can trigger it once the deadline + grace period elapses, but
        /// funds only ever go to the contributors on record (#42).
        ///
        /// Before the deadline + grace period, only the admin may cancel (via
        /// `cancel_milestone`). After it, this function requires no
        /// authorization at all — protecting sponsors against an unresponsive
        /// admin.
        pub fn cancel_milestone_after_deadline(env: Env, milestone_id: u64) -> Result<(), Error> {
            let mkey = DataKey::Milestone(milestone_id);
            let mut milestone: Milestone = env
                .storage()
                .persistent()
                .get(&mkey)
                .ok_or(Error::MilestoneNotFound)?;

            if milestone.closed {
                return Err(Error::MilestoneClosed);
            }

            let now = env.ledger().timestamp();
            if now < milestone.deadline.saturating_add(GRACE_PERIOD) {
                return Err(Error::DeadlineNotPassed);
            }

            let refunded_amount = milestone.remaining_budget;
            if milestone.remaining_budget > 0 {
                refund_remaining_budget(&env, milestone_id, &milestone)?;
                milestone.remaining_budget = 0;
            }
            milestone.closed = true;
            env.storage().persistent().set(&mkey, &milestone);
            extend_ttl(&env, &mkey);
            extend_instance_ttl(&env);
            MilestoneCancelled {
                milestone_id,
                refunded_amount,
                permissionless: true,
            }
            .publish(&env);
            Ok(())
        }

        /// Permissionless TTL refresh: re-extends `milestone_id`'s
        /// persistent-storage TTL (and those of all its `Contribution`
        /// sub-records) by the standard flat bump, without touching any
        /// milestone state. Exists because individual contribution entries
        /// have their own TTL and will archive independently of the parent
        /// `Milestone` record if never re-extended — for a long-lived
        /// milestone whose mutating calls (allocate/release_issue) don't touch
        /// older contributions, those records can silently fall off-ledger.
        ///
        /// Unlike `escrow::keep_alive`, milestones have no natural deadline
        /// timestamp, so this applies the flat ~29-day bump rather than a
        /// deadline-scaled extension. Call it periodically (at least once
        /// within any ~29-day window) to keep a long-lived milestone and all
        /// its contribution history alive.
        ///
        /// Callable by anyone and needs no authorization: it can only ever keep
        /// records alive longer, never change what they hold.
        pub fn keep_alive(env: Env, milestone_id: u64) -> Result<(), Error> {
            let mkey = DataKey::Milestone(milestone_id);
            let milestone: Milestone = env
                .storage()
                .persistent()
                .get(&mkey)
                .ok_or(Error::MilestoneNotFound)?;

            extend_ttl(&env, &mkey);

            // Keep every contribution sub-record alive alongside the parent so
            // they can't archive independently while the milestone stays live.
            for i in 0..milestone.contributor_count {
                extend_ttl(&env, &DataKey::Contribution(milestone_id, i));
            }

            // Keep every IssueStatus record alive: an allocated issue whose TTL
            // lapses independently of the parent milestone would appear
            // "unallocated" to release_issue even though the milestone still has
            // the allocation entry — the two records would be permanently out of
            // sync. Refreshing them here closes that window. (#490)
            for issue_id in milestone.allocations.keys() {
                extend_ttl(&env, &DataKey::IssueStatus(milestone_id, issue_id));
            }

            MilestoneKeptAlive { milestone_id }.publish(&env);
            Ok(())
        }

        /// Returns the milestone record for `milestone_id`.
        pub fn get_milestone(env: Env, milestone_id: u64) -> Result<Milestone, Error> {
            env.storage()
                .persistent()
                .get(&DataKey::Milestone(milestone_id))
                .ok_or(Error::MilestoneNotFound)
        }

        /// Returns the admin address stored in instance storage.
        pub fn get_admin(env: Env) -> Result<Address, Error> {
            env.storage()
                .instance()
                .get(&DataKey::Admin)
                .ok_or(Error::NotInitialized)
        }

        /// Returns the treasury address stored in instance storage.
        pub fn get_treasury(env: Env) -> Result<Address, Error> {
            mergefi_common::require_treasury::<DataKey>(&env).ok_or(Error::NotInitialized)
        }

        /// Returns the oracle address stored in instance storage.
        pub fn get_oracle(env: Env) -> Result<Address, Error> {
            env.storage()
                .instance()
                .get(&DataKey::Oracle)
                .ok_or(Error::NotInitialized)
        }

        /// Returns the current contract version.
        pub fn get_version(env: Env) -> u32 {
            env.storage().instance().get(&DataKey::Version).unwrap_or(0)
        }

        /// Admin-authorized rotation: the current admin may set a new admin.
        pub fn set_admin(env: Env, new_admin: Address) -> Result<(), Error> {
            let old_admin = require_admin(&env)?;
            old_admin.require_auth();
            new_admin.require_auth();
            env.storage().instance().set(&DataKey::Admin, &new_admin);
            extend_instance_ttl(&env);
            AdminChanged {
                old_admin,
                new_admin,
            }
            .publish(&env);
            Ok(())
        }

        /// Admin-only: rotate the routine oracle key.
        pub fn set_oracle(env: Env, new_oracle: Address) -> Result<(), Error> {
            let old_oracle = require_oracle(&env)?;
            require_admin(&env)?.require_auth();
            new_oracle.require_auth();
            env.storage().instance().set(&DataKey::Oracle, &new_oracle);
            extend_instance_ttl(&env);
            OracleChanged {
                old_oracle,
                new_oracle,
            }
            .publish(&env);
            Ok(())
        }

        /// Recovery-authorized rotation: if a recovery address was provided at
        /// initialize, that address may appoint a new admin. This covers the
        /// "admin key permanently lost" scenario.
        pub fn recover_admin(env: Env, new_admin: Address) -> Result<(), Error> {
            let recovery: Address = env
                .storage()
                .instance()
                .get(&DataKey::Recovery)
                .ok_or(Error::NotInitialized)?;
            recovery.require_auth();
            new_admin.require_auth();
            env.storage().instance().set(&DataKey::Admin, &new_admin);
            extend_instance_ttl(&env);
            AdminRecovered {
                recovery,
                new_admin,
            }
            .publish(&env);
            Ok(())
        }

        /// Admin-only: rotate treasury. Admin or recovery may be used to change
        /// treasury depending on policy; here we require the current admin.
        pub fn set_treasury(env: Env, new_treasury: Address) -> Result<(), Error> {
            require_admin(&env)?.require_auth();
            let old_treasury =
                mergefi_common::require_treasury::<DataKey>(&env).ok_or(Error::NotInitialized)?;
            env.storage()
                .instance()
                .set(&DataKey::Treasury, &new_treasury);
            extend_instance_ttl(&env);
            TreasuryChanged {
                old_treasury,
                new_treasury,
            }
            .publish(&env);
            Ok(())
        }

        /// Returns the allocation status of `issue_id` within `milestone_id`.
        pub fn get_issue_status(
            env: Env,
            milestone_id: u64,
            issue_id: u64,
        ) -> Result<IssueStatus, Error> {
            env.storage()
                .persistent()
                .get(&DataKey::IssueStatus(milestone_id, issue_id))
                .ok_or(Error::IssueNotAllocated)
        }

        /// Returns the `index`-th contribution recorded for `milestone_id`
        /// (`0` is always the original `create_milestone` caller; subsequent
        /// indices are `contribute` calls in the order they were accepted),
        /// letting off-chain callers enumerate the full contribution ledger
        /// via `0..milestone.contributor_count`.
        pub fn get_contribution(
            env: Env,
            milestone_id: u64,
            index: u32,
        ) -> Result<Contribution, Error> {
            if !env
                .storage()
                .persistent()
                .has(&DataKey::Milestone(milestone_id))
            {
                return Err(Error::MilestoneNotFound);
            }
            env.storage()
                .persistent()
                .get(&DataKey::Contribution(milestone_id, index))
                .ok_or(Error::ContributionNotFound)
        }

        /// Returns every contribution recorded for `milestone_id` in one call
        /// (issue #146) — `0..milestone.contributor_count`, in the same order
        /// `get_contribution` exposes individually. Bounded by `MAX_SPONSORS`
        /// (20), so this is always a small, cheap read.
        pub fn get_contributions(env: Env, milestone_id: u64) -> Result<Vec<Contribution>, Error> {
            let milestone = Self::get_milestone(env.clone(), milestone_id)?;
            let mut contributions = Vec::new(&env);
            for i in 0..milestone.contributor_count {
                let contribution: Contribution = env
                    .storage()
                    .persistent()
                    .get(&DataKey::Contribution(milestone_id, i))
                    .ok_or(Error::MilestoneNotFound)?;
                contributions.push_back(contribution);
            }
            Ok(contributions)
        }

        /// Returns the fee basis points stored in instance storage.
        pub fn get_fee_bps(env: Env) -> Result<u32, Error> {
            mergefi_common::get_fee_bps::<DataKey>(&env).ok_or(Error::NotInitialized)
        }

        /// Returns the maximum number of sponsors allowed per milestone.
        pub fn get_max_sponsors(env: Env) -> Result<u32, Error> {
            env.storage()
                .instance()
                .get(&DataKey::MaxSponsors)
                .ok_or(Error::NotInitialized)
        }

        /// Returns the grace period in seconds before permissionless milestone cancellation.
        pub fn get_grace_period(_env: Env) -> u64 {
            GRACE_PERIOD
        }

        /// Returns the basis-point denominator used for fees and payouts.
        pub fn get_bps_denominator(_env: Env) -> i128 {
            BPS_DENOMINATOR
        }

        /// Admin-only: update the protocol fee for NEW milestones created after
        /// this call (Issue #20). Existing milestones are unaffected - they retain
        /// the fee that was active when they were created, providing sponsor trust
        /// and predictability.
        ///
        /// The change is limited to 5% (500 basis points) per call to prevent
        /// accidental or malicious fee spikes.
        pub fn set_fee_bps(env: Env, new_fee_bps: u32) -> Result<(), Error> {
            require_admin(&env)?.require_auth();

            let current_fee: u32 =
                mergefi_common::get_fee_bps::<DataKey>(&env).ok_or(Error::NotInitialized)?;

            mergefi_common::validate_fee_change(current_fee, new_fee_bps)
                .map_err(|_| Error::InvalidFee)?;

            env.storage().instance().set(&DataKey::FeeBps, &new_fee_bps);
            extend_instance_ttl(&env);
            FeeBpsChanged {
                old_fee_bps: current_fee,
                new_fee_bps,
            }
            .publish(&env);
            Ok(())
        }
    }
} // mod contract

pub use contract::*;

/// Refunds each contributor their actual unspent remainder from the
/// milestone's remaining budget. Each contributor's refund is exactly their
/// `contribution.remaining` amount, which has been properly tracked through
/// FIFO deductions during `release_issue` calls. The sum of all `remaining`
/// values across contributions equals `milestone.remaining_budget`, so all
/// funds are returned with no dust left in the contract.
///
/// Before this fix (issue #750), refunds incorrectly used
/// `remaining_budget * contribution.amount / total_budget`, which unfairly
/// penalized late contributors by refunding them a share of money that had
/// already been spent by earlier contributors. The FIFO tracking ensures
/// each sponsor gets back exactly what wasn't spent from their contribution.
fn refund_remaining_budget(
    env: &Env,
    milestone_id: u64,
    milestone: &Milestone,
) -> Result<(), Error> {
    let remaining = milestone.remaining_budget;
    if remaining <= 0 {
        return Ok(());
    }

    let token_client = token::Client::new(env, &milestone.token);
    let contract_address = env.current_contract_address();

    // Refund each contributor their actual unspent remainder
    for i in 0..milestone.contributor_count {
        let contribution_key = DataKey::Contribution(milestone_id, i);
        let contribution: Contribution = env
            .storage()
            .persistent()
            .get(&contribution_key)
            .ok_or(Error::MilestoneNotFound)?;
        
        if contribution.remaining > 0 {
            token_client.transfer(&contract_address, &contribution.sponsor, &contribution.remaining);
            MilestoneRefundPaid {
                milestone_id,
                sponsor: contribution.sponsor,
                amount: contribution.remaining,
            }
            .publish(env);
        }
    }

    Ok(())
}

fn require_admin(env: &Env) -> Result<Address, Error> {
    mergefi_common::require_admin::<DataKey>(env).ok_or(Error::NotInitialized)
}

fn require_oracle(env: &Env) -> Result<Address, Error> {
    mergefi_common::require_oracle::<DataKey>(env).ok_or(Error::NotInitialized)
}

fn is_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

fn extend_ttl(env: &Env, key: &DataKey) {
    mergefi_common::extend_ttl(env, key);
}
