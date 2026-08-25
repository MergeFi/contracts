//! MergeFi Escrow Contract
//!
//! Single-issue escrow with deadline-based refunds.
//! Supports multiple sponsors co-funding a single issue.

#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Vec, Map, Symbol, symbol_short};

mod error;
mod test;
mod types;

use error::Error;
use types::{Escrow, Contribution, EscrowStatus};

const DAY_IN_LEDGERS: u32 = 17280;
const INSTANCE_BUMP: u32 = 7 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME: u32 = 30 * DAY_IN_LEDGERS;
const MAX_CONTRIBUTORS: u32 = 100;

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    /// Initialize the escrow contract with admin and fee configuration.
    pub fn initialize(env: Env, admin: Address, fee_bps: u32, treasury: Address) {
        admin.require_auth();
        if fee_bps > 10000 {
            panic_with_error!(&env, Error::InvalidFeeBps);
        }
        env.storage().instance().set(&symbol_short!("admin"), &admin);
        env.storage().instance().set(&symbol_short!("fee_bps"), &fee_bps);
        env.storage().instance().set(&symbol_short!("treasury"), &treasury);
        env.storage().instance().extend_ttl(INSTANCE_LIFETIME, INSTANCE_BUMP);
    }

    /// Fund an escrow for a specific issue.
    /// Multiple sponsors can contribute to the same issue_id.
    /// Total contributions cannot exceed the target amount.
    pub fn fund(
        env: Env,
        sponsor: Address,
        issue_id: u64,
        token: Address,
        amount: i128,
        deadline: u64,
    ) -> Result<(), Error> {
        sponsor.require_auth();

        if amount <= 0 {
            panic_with_error!(&env, Error::InvalidAmount);
        }

        let mut escrow: Escrow = match env.storage().persistent().get(&issue_id) {
            Some(e) => e,
            None => {
                // New escrow - validate deadline is in future
                let current_ledger = env.ledger().sequence();
                if deadline <= current_ledger {
                    panic_with_error!(&env, Error::DeadlineInPast);
                }
                Escrow {
                    issue_id,
                    token,
                    target_amount: amount,
                    contributions: Vec::new(&env),
                    status: EscrowStatus::Funding,
                    deadline,
                    admin: env.storage().instance().get(&symbol_short!("admin")).unwrap(),
                    fee_bps: env.storage().instance().get(&symbol_short!("fee_bps")).unwrap(),
                    treasury: env.storage().instance().get(&symbol_short!("treasury")).unwrap(),
                }
            }
        };

        // Check if escrow is still in funding state
        if escrow.status != EscrowStatus::Funding {
            panic_with_error!(&env, Error::NotInFundingState);
        }

        // Check deadline hasn't passed
        let current_ledger = env.ledger().sequence();
        if deadline <= current_ledger {
            panic_with_error!(&env, Error::DeadlineInPast);
        }

        // For existing escrow, deadline must match
        if escrow.deadline != deadline {
            panic_with_error!(&env, Error::DeadlineMismatch);
        }

        // Check token matches
        if escrow.token != token {
            panic_with_error!(&env, Error::TokenMismatch);
        }

        // Calculate current total funded
        let current_total: i128 = escrow.contributions.iter().map(|c| c.amount).sum();
        let new_total = current_total.checked_add(amount).ok_or(Error::MathOverflow)?;

        if new_total > escrow.target_amount {
            panic_with_error!(&env, Error::ExceedsTargetAmount);
        }

        // Transfer tokens from sponsor
        let token_client = soroban_sdk::token::Client::new(&env, &token);
        token_client.transfer(&sponsor, &env.current_contract_address(), &amount);

        // Add or update contribution
        let mut found = false;
        for i in 0..escrow.contributions.len() {
            let mut contrib = escrow.contributions.get(i).unwrap();
            if contrib.sponsor == sponsor {
                contrib.amount = contrib.amount.checked_add(amount).ok_or(Error::MathOverflow)?;
                escrow.contributions.set(i, contrib);
                found = true;
                break;
            }
        }
        if !found {
            // Check contribution limit to prevent unbounded growth
            if escrow.contributions.len() >= MAX_CONTRIBUTORS {
                panic_with_error!(&env, Error::TooManyContributors);
            }
            escrow.contributions.push_back(Contribution {
                sponsor,
                amount,
            });
        }

        // Update status if fully funded
        if new_total == escrow.target_amount {
            escrow.status = EscrowStatus::Funded;
        }

        env.storage().persistent().set(&issue_id, &escrow);
        env.storage().persistent().extend_ttl(&issue_id, INSTANCE_LIFETIME, INSTANCE_BUMP);

        Ok(())
    }

    /// Release funds to recipients (called by admin/oracle).
    /// Distributes funds according to basis-point splits, deducts protocol fee.
    pub fn release(
        env: Env,
        issue_id: u64,
        recipients: Vec<(Address, u32)>, // (address, basis_points)
    ) -> Result<(), Error> {
        let admin: Address = env.storage().instance().get(&symbol_short!("admin")).unwrap();
        admin.require_auth();

        let mut escrow: Escrow = env.storage().persistent().get(&issue_id).ok_or(Error::EscrowNotFound)?;

        if escrow.status != EscrowStatus::Funded {
            panic_with_error!(&env, Error::NotFullyFunded);
        }

        // Validate recipients and total basis points
        let total_bps: u32 = recipients.iter().map(|r| r.1).sum();
        if total_bps != 10000 {
            panic_with_error!(&env, Error::InvalidSplit);
        }
        if recipients.len() == 0 {
            panic_with_error!(&env, Error::NoRecipients);
        }
        if recipients.len() > 50 {
            panic_with_error!(&env, Error::TooManyRecipients);
        }

        let token_client = soroban_sdk::token::Client::new(&env, &escrow.token);
        let total_amount = escrow.target_amount;
        let fee_amount = (total_amount * escrow.fee_bps as i128) / 10000;
        let distributable = total_amount - fee_amount;

        // Compute splits using largest remainder method
        let splits = compute_split(distributable, &recipients);

        // Pay each recipient
        for (recipient, amount) in splits.iter() {
            if amount > &0 {
                token_client.transfer(&env.current_contract_address(), recipient, amount);
            }
        }

        // Pay fee to treasury
        if fee_amount > 0 {
            token_client.transfer(&env.current_contract_address(), &escrow.treasury, &fee_amount);
        }

        escrow.status = EscrowStatus::Released;
        env.storage().persistent().set(&issue_id, &escrow);
        env.storage().persistent().extend_ttl(&issue_id, INSTANCE_LIFETIME, INSTANCE_BUMP);

        Ok(())
    }

    /// Refund all contributors proportionally.
    /// Can be called by admin at any time, or by anyone after deadline expires.
    pub fn refund(env: Env, issue_id: u64, caller: Address) -> Result<(), Error> {
        let mut escrow: Escrow = env.storage().persistent().get(&issue_id).ok_or(Error::EscrowNotFound)?;

        if escrow.status == EscrowStatus::Released || escrow.status == EscrowStatus::Refunded {
            panic_with_error!(&env, Error::AlreadySettled);
        }

        let current_ledger = env.ledger().sequence();
        let admin: Address = env.storage().instance().get(&symbol_short!("admin")).unwrap();

        // Check authorization: admin can refund anytime, anyone can refund after deadline
        if caller != admin {
            if current_ledger <= escrow.deadline {
                panic_with_error!(&env, Error::DeadlineNotExpired);
            }
            caller.require_auth();
        } else {
            admin.require_auth();
        }

        let token_client = soroban_sdk::token::Client::new(&env, &escrow.token);

        // Refund each contributor their exact contribution amount
        for contrib in escrow.contributions.iter() {
            if contrib.amount > 0 {
                token_client.transfer(&env.current_contract_address(), &contrib.sponsor, &contrib.amount);
            }
        }

        escrow.status = EscrowStatus::Refunded;
        env.storage().persistent().set(&issue_id, &escrow);
        env.storage().persistent().extend_ttl(&issue_id, INSTANCE_LIFETIME, INSTANCE_BUMP);

        Ok(())
    }

    /// Extend the deadline for an escrow.
    /// Requires unanimous consent from all contributors.
    /// Each contributor must call this function with the same new_deadline.
    pub fn extend_deadline(
        env: Env,
        issue_id: u64,
        new_deadline: u64,
        sponsor: Address,
    ) -> Result<(), Error> {
        sponsor.require_auth();

        let mut escrow: Escrow = env.storage().persistent().get(&issue_id).ok_or(Error::EscrowNotFound)?;

        if escrow.status != EscrowStatus::Funding && escrow.status != EscrowStatus::Funded {
            panic_with_error!(&env, Error::NotInFundingState);
        }

        let current_ledger = env.ledger().sequence();
        if new_deadline <= current_ledger {
            panic_with_error!(&env, Error::DeadlineInPast);
        }
        if new_deadline <= escrow.deadline {
            panic_with_error!(&env, Error::DeadlineNotExtended);
        }

        // Verify sponsor is a contributor
        let mut is_contributor = false;
        for contrib in escrow.contributions.iter() {
            if contrib.sponsor == sponsor {
                is_contributor = true;
                break;
            }
        }
        if !is_contributor {
            panic_with_error!(&env, Error::NotAContributor);
        }

        // Track approvals for this extension
        let approval_key = (issue_id, new_deadline);
        let mut approvals: Map<Address, bool> = env.storage().persistent().get(&approval_key).unwrap_or(Map::new(&env));
        
        approvals.set(sponsor.clone(), true);
        env.storage().persistent().set(&approval_key, &approvals);
        env.storage().persistent().extend_ttl(&approval_key, INSTANCE_LIFETIME, INSTANCE_BUMP);

        // Check if all contributors have approved
        let mut all_approved = true;
        for contrib in escrow.contributions.iter() {
            if !approvals.get(contrib.sponsor).unwrap_or(false) {
                all_approved = false;
                break;
            }
        }

        if all_approved {
            escrow.deadline = new_deadline;
            env.storage().persistent().set(&issue_id, &escrow);
            env.storage().persistent().extend_ttl(&issue_id, INSTANCE_LIFETIME, INSTANCE_BUMP);
            // Clean up approvals
            env.storage().persistent().remove(&approval_key);
        }

        Ok(())
    }

    /// Get escrow details.
    pub fn get_escrow(env: Env, issue_id: u64) -> Result<Escrow, Error> {
        env.storage().persistent().get(&issue_id).ok_or(Error::EscrowNotFound)
    }

    /// Get contribution details for a specific sponsor.
    pub fn get_contribution(env: Env, issue_id: u64, sponsor: Address) -> Result<i128, Error> {
        let escrow: Escrow = env.storage().persistent().get(&issue_id).ok_or(Error::EscrowNotFound)?;
        for contrib in escrow.contributions.iter() {
            if contrib.sponsor == sponsor {
                return Ok(contrib.amount);
            }
        }
        Ok(0)
    }
}

/// Compute token splits using largest remainder method.
/// Returns vector of (recipient, amount) pairs.
fn compute_split(distributable: i128, recipients: &Vec<(Address, u32)>) -> Vec<(Address, i128)> {
    let env = recipients.env();
    let mut splits = Vec::new(&env);
    let mut remainders = Vec::new(&env);
    let mut allocated = 0i128;

    // First pass: floor allocation
    for (recipient, bps) in recipients.iter() {
        let amount = (distributable * bps as i128) / 10000;
        splits.push_back((recipient.clone(), amount));
        allocated += amount;
        let remainder = (distributable * bps as i128) % 10000;
        remainders.push_back((recipient.clone(), remainder));
    }

    // Second pass: distribute remainder by largest remainder
    let remaining = distributable - allocated;
    if remaining > 0 {
        // Sort by remainder descending (simple bubble sort for small n)
        let mut rem_vec: Vec<(Address, i128)> = Vec::new(&env);
        for r in remainders.iter() {
            rem_vec.push_back(r);
        }
        // Simple sort for small collections
        for i in 0..rem_vec.len() {
            for j in i + 1..rem_vec.len() {
                let ri = rem_vec.get(i).unwrap();
                let rj = rem_vec.get(j).unwrap();
                if ri.1 < rj.1 {
                    rem_vec.set(i, rj);
                    rem_vec.set(j, ri);
                }
            }
        }
        
        for i in 0..remaining.min(rem_vec.len() as i128) as u32 {
            let (recipient, _) = rem_vec.get(i).unwrap();
            for j in 0..splits.len() {
                let (r, amt) = splits.get(j).unwrap();
                if r == recipient {
                    splits.set(j, (r, amt + 1));
                    break;
                }
            }
        }
    }

    splits
}
