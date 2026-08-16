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
    /// One-time setup. Requires `admin`'s own authorization, so nobody can
    /// name a third-party address as admin without that address's consent
    /// — see `docs/access-control-audit.md` for what this does and does
    /// not protect against (it does not stop initializer front-running).
    pub fn initialize(
        env: Env,
        admin: Address,
        treasury: Address,
        fee_bps: u32,
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
    let mut order: Vec<(u32, i128, Address)> = Vec::new(env);
    let mut allocated: i128 = 0;

    for (recipient, bps) in recipients.iter() {
        let numerator = distributable * (bps as i128);
        let share = numerator / BPS_DENOMINATOR;
        let remainder = numerator % BPS_DENOMINATOR;
        allocated += share;
        shares.push_back((recipient.clone(), share));
        order.push_back((order.len(), remainder, recipient));
    }

    // Distribute the rounding dust by largest remainder (with the existing
    // address-based tie-break) in O(n log n): sort the (index, remainder,
    // address) records once, then award one unit to each of the first `dust`
    // entries. This is equivalent to the previous repeated-linear-scan loop,
    // because each award only consumes the selected entry and never changes
    // any other entry's remainder. `dust` is at most `recipients.len() - 1`,
    // so the first `dust` sorted entries always exist.
    let dust = distributable - allocated;
    if dust > 0 {
        sort_remainders_desc(&mut order);
        for k in 0..dust as u32 {
            let (index, _, _) = order.get(k).unwrap();
            let (recipient, share) = shares.get(index).unwrap();
            shares.set(index, (recipient, share + 1));
        }
    }

    Ok(Payouts { fee, shares })
}

/// True if `a` sorts before `b` in largest-remainder order: remainder
/// descending, then address ascending, then original index ascending (which
/// reproduces the address-based tie-break of the previous O(n²) loop).
fn remainder_order_less(a: &(u32, i128, Address), b: &(u32, i128, Address)) -> bool {
    b.1.cmp(&a.1)
        .then_with(|| a.2.cmp(&b.2))
        .then_with(|| a.0.cmp(&b.0))
        == core::cmp::Ordering::Less
}

/// Sifts the element at `start` down a max-heap occupying `[start, end)`,
/// ordering elements by [`remainder_order_less`].
fn sift_down_remainder_order(order: &mut Vec<(u32, i128, Address)>, start: u32, end: u32) {
    let mut root = start;
    loop {
        let mut child = 2 * root + 1;
        if child >= end {
            break;
        }
        if child + 1 < end
            && remainder_order_less(&order.get(child).unwrap(), &order.get(child + 1).unwrap())
        {
            child += 1;
        }
        if remainder_order_less(&order.get(root).unwrap(), &order.get(child).unwrap()) {
            let a = order.get(root).unwrap();
            let b = order.get(child).unwrap();
            order.set(root, b);
            order.set(child, a);
            root = child;
        } else {
            break;
        }
    }
}

/// In-place heapsort of `(index, remainder, address)` records into
/// largest-remainder order. O(n log n) worst case, with no recursion and no
/// heap allocation, so it is safe under `#![no_std]` and only mutates the
/// host-backed `order` through `get`/`set`.
fn sort_remainders_desc(order: &mut Vec<(u32, i128, Address)>) {
    let n = order.len();
    if n < 2 {
        return;
    }

    // Build a max-heap over the whole array.
    let mut start = n / 2;
    loop {
        start -= 1;
        sift_down_remainder_order(order, start, n);
        if start == 0 {
            break;
        }
    }

    // Repeatedly move the largest remaining element to the end of the array,
    // shrinking the heap until the array is sorted ascending by `less`.
    let mut end = n;
    while end > 1 {
        end -= 1;
        let a = order.get(0).unwrap();
        let b = order.get(end).unwrap();
        order.set(0, b);
        order.set(end, a);
        sift_down_remainder_order(order, 0, end);
    }
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
