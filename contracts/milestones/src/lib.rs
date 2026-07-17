//! MergeFi Milestone Funding Contract
//!
//! A milestone pools a sponsor's lump-sum budget across multiple GitHub
//! issues that make up a release. The sponsor deposits once; the backend
//! oracle allocates slices of the budget to individual issues and later
//! releases each allocation (optionally split across a team) as issues are
//! merged, exactly like the escrow contract's `release`, but drawn from a
//! shared pool instead of a single-issue deposit.
#![no_std]

mod error;
mod types;

#[cfg(test)]
mod test;

use error::Error;
use soroban_sdk::{contract, contractimpl, token, Address, Env, Map, Vec};
use types::{DataKey, IssueStatus, Milestone};

pub const BPS_DENOMINATOR: i128 = 10_000;

#[contract]
pub struct MilestonesContract;

#[contractimpl]
impl MilestonesContract {
    pub fn initialize(
        env: Env,
        admin: Address,
        treasury: Address,
        fee_bps: u32,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        if fee_bps as i128 > BPS_DENOMINATOR {
            return Err(Error::InvalidFee);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
        Ok(())
    }

    /// Sponsor deposits `total_budget` of `token` to open a new milestone
    /// pool. Requires sponsor authorization.
    pub fn create_milestone(
        env: Env,
        milestone_id: u64,
        sponsor: Address,
        token: Address,
        total_budget: i128,
    ) -> Result<(), Error> {
        sponsor.require_auth();

        if total_budget <= 0 {
            return Err(Error::InvalidAmount);
        }

        let key = DataKey::Milestone(milestone_id);
        if env.storage().persistent().has(&key) {
            return Err(Error::IssueAlreadyAllocated);
        }

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&sponsor, env.current_contract_address(), &total_budget);

        let milestone = Milestone {
            sponsor,
            token,
            total_budget,
            remaining_budget: total_budget,
            created_at: env.ledger().timestamp(),
            closed: false,
            allocations: Map::new(&env),
        };
        env.storage().persistent().set(&key, &milestone);
        extend_ttl(&env, &key);
        Ok(())
    }

    /// Admin-only: reserves `amount` of the milestone's remaining budget for
    /// `issue_id`. Rejects if the issue is already allocated, the milestone
    /// is closed, or `amount` exceeds the remaining (unallocated) budget.
    pub fn allocate(env: Env, milestone_id: u64, issue_id: u64, amount: i128) -> Result<(), Error> {
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
        if milestone.allocations.contains_key(issue_id) {
            return Err(Error::IssueAlreadyAllocated);
        }
        if amount > milestone.remaining_budget {
            return Err(Error::OverAllocation);
        }

        milestone.remaining_budget -= amount;
        milestone.allocations.set(issue_id, amount);
        env.storage().persistent().set(&mkey, &milestone);
        extend_ttl(&env, &mkey);

        let skey = DataKey::IssueStatus(milestone_id, issue_id);
        env.storage()
            .persistent()
            .set(&skey, &IssueStatus::Allocated);
        extend_ttl(&env, &skey);

        Ok(())
    }

    /// Admin-only: releases the previously allocated amount for `issue_id`
    /// to `recipients` (basis points summing to 10000), minus the protocol
    /// fee, exactly as in the escrow contract.
    pub fn release_issue(
        env: Env,
        milestone_id: u64,
        issue_id: u64,
        recipients: Vec<(Address, u32)>,
    ) -> Result<(), Error> {
        require_admin(&env)?.require_auth();

        let mkey = DataKey::Milestone(milestone_id);
        let milestone: Milestone = env
            .storage()
            .persistent()
            .get(&mkey)
            .ok_or(Error::MilestoneNotFound)?;

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

        let payouts = compute_split(&env, amount, &recipients)?;
        let treasury: Address = env.storage().instance().get(&DataKey::Treasury).unwrap();
        let token_client = token::Client::new(&env, &milestone.token);
        let contract_address = env.current_contract_address();

        if payouts.fee > 0 {
            token_client.transfer(&contract_address, &treasury, &payouts.fee);
        }
        for (recipient, share) in payouts.shares.iter() {
            if share > 0 {
                token_client.transfer(&contract_address, &recipient, &share);
            }
        }

        env.storage()
            .persistent()
            .set(&skey, &IssueStatus::Released);
        extend_ttl(&env, &skey);

        Ok(())
    }

    /// Admin-only: closes the milestone and refunds any unallocated budget
    /// back to the sponsor (e.g. release cancelled with issues remaining).
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

        if milestone.remaining_budget > 0 {
            let token_client = token::Client::new(&env, &milestone.token);
            token_client.transfer(
                &env.current_contract_address(),
                &milestone.sponsor,
                &milestone.remaining_budget,
            );
            milestone.remaining_budget = 0;
        }
        milestone.closed = true;
        env.storage().persistent().set(&mkey, &milestone);
        extend_ttl(&env, &mkey);
        Ok(())
    }

    pub fn get_milestone(env: Env, milestone_id: u64) -> Result<Milestone, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Milestone(milestone_id))
            .ok_or(Error::MilestoneNotFound)
    }

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
}

struct Payouts {
    fee: i128,
    shares: Vec<(Address, i128)>,
}

fn compute_split(
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

    // Largest-remainder allocation: the floor-division shortfall is always
    // smaller than recipients.len(), and assigning it by fractional remainder
    // removes the previous position-dependent "last recipient gets dust" bias.
    let mut dust = distributable - allocated;
    while dust > 0 {
        let mut best_index: u32 = 0;
        let mut best_remainder: i128 = -1;
        for (i, remainder) in remainders.iter().enumerate() {
            if remainder > best_remainder {
                best_index = i as u32;
                best_remainder = remainder;
            }
        }

        let (recipient, share) = shares.get(best_index).unwrap();
        shares.set(best_index, (recipient, share + 1));
        remainders.set(best_index, -1);
        dust -= 1;
    }

    Ok(Payouts { fee, shares })
}

fn require_admin(env: &Env) -> Result<Address, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotInitialized)
}

fn extend_ttl(env: &Env, key: &DataKey) {
    env.storage().persistent().extend_ttl(key, 100_000, 500_000);
}
