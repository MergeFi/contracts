//! MergeFi Recurring Maintenance Pool Contract
//!
//! Unlike the escrow contract (funds tied to a single issue) or the
//! milestones contract (a lump sum allocated across issues in a release),
//! a maintenance pool is a standing balance tied to a repository or org.
//! Sponsors can deposit into it repeatedly over time; maintainers draw
//! down rewards for ongoing maintenance-type work as authorized by the
//! backend oracle, which tracks off-chain maintenance activity.
#![no_std]

mod error;
mod types;

#[cfg(test)]
mod test;

use error::Error;
use soroban_sdk::{contract, contractimpl, token, Address, Env};
use types::{DataKey, Deposit, MaintenancePool};

pub const BPS_DENOMINATOR: i128 = 10_000;

#[contract]
pub struct MaintenancePoolContract;

#[contractimpl]
impl MaintenancePoolContract {
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

    /// Sponsor deposits `amount` of `token` into the pool identified by
    /// `pool_id` (an off-chain-assigned id for a repo/org). Creates the
    /// pool on first deposit; subsequent deposits must use the same token.
    /// Requires sponsor authorization. Every deposit is recorded so the
    /// full contribution history can be queried.
    pub fn deposit(
        env: Env,
        pool_id: u64,
        sponsor: Address,
        token: Address,
        amount: i128,
    ) -> Result<(), Error> {
        sponsor.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let pkey = DataKey::Pool(pool_id);
        let mut pool: MaintenancePool = match env.storage().persistent().get(&pkey) {
            Some(p) => p,
            None => MaintenancePool {
                token: token.clone(),
                balance: 0,
                total_deposited: 0,
                total_withdrawn: 0,
                created_at: env.ledger().timestamp(),
                deposit_count: 0,
            },
        };

        if pool.deposit_count > 0 && pool.token != token {
            return Err(Error::TokenMismatch);
        }

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&sponsor, &env.current_contract_address(), &amount);

        pool.balance += amount;
        pool.total_deposited += amount;
        let index = pool.deposit_count;
        pool.deposit_count += 1;

        env.storage().persistent().set(&pkey, &pool);
        extend_ttl(&env, &pkey);

        let dkey = DataKey::Deposit(pool_id, index);
        env.storage().persistent().set(
            &dkey,
            &Deposit {
                sponsor,
                amount,
                timestamp: env.ledger().timestamp(),
            },
        );
        extend_ttl(&env, &dkey);

        Ok(())
    }

    /// Admin-only: pays `amount` (minus the protocol fee) out of the pool
    /// to `recipient` (a maintainer), as authorized off-chain by the
    /// backend oracle for completed maintenance work. Rejects if the pool
    /// balance is insufficient.
    pub fn withdraw(
        env: Env,
        pool_id: u64,
        recipient: Address,
        amount: i128,
    ) -> Result<(), Error> {
        require_admin(&env)?.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let pkey = DataKey::Pool(pool_id);
        let mut pool: MaintenancePool = env
            .storage()
            .persistent()
            .get(&pkey)
            .ok_or(Error::PoolNotFound)?;

        if amount > pool.balance {
            return Err(Error::InsufficientBalance);
        }

        let fee_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::FeeBps)
            .ok_or(Error::NotInitialized)?;
        let fee = amount * (fee_bps as i128) / BPS_DENOMINATOR;
        let payout = amount - fee;

        let treasury: Address = env.storage().instance().get(&DataKey::Treasury).unwrap();
        let token_client = token::Client::new(&env, &pool.token);
        let contract_address = env.current_contract_address();

        if fee > 0 {
            token_client.transfer(&contract_address, &treasury, &fee);
        }
        token_client.transfer(&contract_address, &recipient, &payout);

        pool.balance -= amount;
        pool.total_withdrawn += amount;
        env.storage().persistent().set(&pkey, &pool);
        extend_ttl(&env, &pkey);

        Ok(())
    }

    pub fn get_pool(env: Env, pool_id: u64) -> Result<MaintenancePool, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Pool(pool_id))
            .ok_or(Error::PoolNotFound)
    }

    pub fn get_deposit(env: Env, pool_id: u64, index: u32) -> Result<Deposit, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Deposit(pool_id, index))
            .ok_or(Error::PoolNotFound)
    }
}

fn require_admin(env: &Env) -> Result<Address, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotInitialized)
}

fn extend_ttl(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, 100_000, 500_000);
}
