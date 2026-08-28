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

use mergefi_common::BPS_DENOMINATOR;

/// Inactivity window (in seconds) after which a deposit becomes
/// permissionlessly reclaimable by its original sponsor (#42). If no
/// `withdraw` occurs against the pool for this duration, any sponsor
/// can reclaim their own deposit. This mirrors escrow's GRACE_PERIOD
/// concept but applied per-deposit rather than per-pool.
pub const INACTIVITY_WINDOW: u64 = 90 * 24 * 60 * 60; // 90 days

#[contract]
pub struct MaintenancePoolContract;

#[contractimpl]
impl MaintenancePoolContract {
    /// One-time setup. Requires `admin`'s own authorization, so nobody can
    /// name a third-party address as admin without that address's consent
    /// — see `docs/access-control-audit.md` for what this does and does
    /// not protect against (it does not stop initializer front-running).
    pub fn initialize(
        env: Env,
        admin: Address,
        treasury: Address,
        fee_bps: u32,
        recovery: Option<Address>,
    ) -> Result<(), Error> {
        admin.require_auth();

        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        if fee_bps as i128 > BPS_DENOMINATOR {
            return Err(Error::InvalidFee);
        }
        if treasury == env.current_contract_address() {
            return Err(Error::InvalidTreasury);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
        if let Some(r) = recovery {
            env.storage().instance().set(&DataKey::Recovery, &r);
        }
        extend_instance_ttl(&env);
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
                last_withdraw_at: 0,
            },
        };

        if pool.deposit_count > 0 && pool.token != token {
            return Err(Error::TokenMismatch);
        }

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&sponsor, env.current_contract_address(), &amount);

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

        // Refresh every prior deposit sub-record so older entries don't
        // archive while the pool stays active with recurring new deposits.
        // The newly-written record at `index` was already extended above;
        // the loop covers the full range so all historical records are refreshed.
        for i in 0..pool.deposit_count {
            extend_ttl(&env, &DataKey::Deposit(pool_id, i));
        }
        extend_instance_ttl(&env);

        Ok(())
    }

    /// Admin-only: pays `amount` (minus the protocol fee) out of the pool
    /// to `recipient` (a maintainer), as authorized off-chain by the
    /// backend oracle for completed maintenance work. Rejects if the pool
    /// balance is insufficient.
    pub fn withdraw(env: Env, pool_id: u64, recipient: Address, amount: i128) -> Result<(), Error> {
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
        pool.last_withdraw_at = env.ledger().timestamp();
        env.storage().persistent().set(&pkey, &pool);
        extend_ttl(&env, &pkey);

        // Refresh all deposit sub-records on every withdrawal so historical
        // deposit records stay queryable across a long-running pool's lifetime.
        for i in 0..pool.deposit_count {
            extend_ttl(&env, &DataKey::Deposit(pool_id, i));
        }
        extend_instance_ttl(&env);

        Ok(())
    }

    /// Permissionless deposit reclaim after the pool's inactivity window
    /// has elapsed (#42). If no `withdraw` has occurred against the pool
    /// for `INACTIVITY_WINDOW` seconds, any original sponsor can reclaim
    /// their own deposit — providing a non-admin-gated recovery path for
    /// sponsors whose pool admin has gone permanently unresponsive.
    ///
    /// The inactivity window resets every time `withdraw` is called, so
    /// an actively-managed pool is never affected. Only the original
    /// deposit sponsor can reclaim their own deposit; funds always return
    /// to the address on record, never to an arbitrary caller.
    ///
    /// Rejects if:
    /// - The pool doesn't exist
    /// - The deposit index is out of range
    /// - The caller is not the deposit's original sponsor
    /// - The inactivity window hasn't elapsed since the last withdrawal
    /// - The pool balance is insufficient (partial reclaim not supported)
    pub fn reclaim_deposit(
        env: Env,
        pool_id: u64,
        deposit_index: u32,
        sponsor: Address,
    ) -> Result<(), Error> {
        sponsor.require_auth();

        let pkey = DataKey::Pool(pool_id);
        let mut pool: MaintenancePool = env
            .storage()
            .persistent()
            .get(&pkey)
            .ok_or(Error::PoolNotFound)?;

        let dkey = DataKey::Deposit(pool_id, deposit_index);
        let deposit: Deposit = env
            .storage()
            .persistent()
            .get(&dkey)
            .ok_or(Error::DepositNotFound)?;

        if deposit.sponsor != sponsor {
            return Err(Error::NotDepositSponsor);
        }

        let now = env.ledger().timestamp();
        if now < pool.last_withdraw_at + INACTIVITY_WINDOW {
            return Err(Error::InactivityWindowNotElapsed);
        }

        if deposit.amount > pool.balance {
            return Err(Error::InsufficientBalance);
        }

        let token_client = token::Client::new(&env, &pool.token);
        token_client.transfer(
            &env.current_contract_address(),
            &sponsor,
            &deposit.amount,
        );

        pool.balance -= deposit.amount;
        env.storage().persistent().set(&pkey, &pool);
        extend_ttl(&env, &pkey);
        extend_instance_ttl(&env);

        Ok(())
    }

    /// Permissionless TTL refresh: re-extends `pool_id`'s persistent-storage
    /// TTL (and those of all its `Deposit` sub-records) by the standard flat
    /// bump, without touching any pool state. Exists because individual
    /// deposit entries have their own TTL and will archive independently of
    /// the parent `MaintenancePool` record if never re-extended.
    ///
    /// This matters most for the maintenance pool because it is explicitly
    /// designed to be open-ended and long-lived ("it never finishes") — the
    /// contract with the longest expected lifetime also has the largest
    /// accumulation of historical deposit records, each of which needs its
    /// own TTL refreshed to stay queryable. Without periodic `keep_alive`
    /// calls, `get_deposit` silently breaks for older records even while the
    /// pool itself remains fully active.
    ///
    /// Unlike `escrow::keep_alive`, pools have no deadline timestamp, so
    /// this applies the flat ~29-day bump rather than a deadline-scaled
    /// extension. Call it at least once within any ~29-day window to keep
    /// the full deposit history alive and enumerable.
    ///
    /// Callable by anyone and needs no authorization: it can only ever keep
    /// records alive longer, never change what they hold.
    pub fn keep_alive(env: Env, pool_id: u64) -> Result<(), Error> {
        let pkey = DataKey::Pool(pool_id);
        let pool: MaintenancePool = env
            .storage()
            .persistent()
            .get(&pkey)
            .ok_or(Error::PoolNotFound)?;

        extend_ttl(&env, &pkey);

        // Keep every deposit sub-record alive alongside the parent so the
        // full contribution history advertised by the README stays queryable.
        for i in 0..pool.deposit_count {
            extend_ttl(&env, &DataKey::Deposit(pool_id, i));
        }

        Ok(())
    }

    /// Admin-only function to sweep excess token balances above what is owed
    /// to pool obligations (issue #39). This recovers stray transfers or
    /// direct token sends that aren't accounted for by any pool record.
    ///
    /// The sweep amount is computed as `actual_token_balance - pool.balance`.
    /// This ensures a sweep can never drain funds that are legitimately owed
    /// to sponsors or withdrawable as rewards; it can only recover the true
    /// surplus.
    ///
    /// Requires admin authorization.
    ///
    /// # Arguments
    /// * `pool_id` - The pool identifier to sweep for.
    /// * `token` - The token contract to query balance from.
    /// * `recipient` - Address to send swept tokens to (typically treasury, but
    ///   admin-controlled for flexibility).
    pub fn sweep(
        env: Env,
        pool_id: u64,
        token: Address,
        recipient: Address,
    ) -> Result<i128, Error> {
        require_admin(&env)?;

        let pkey = DataKey::Pool(pool_id);
        let pool: MaintenancePool = env
            .storage()
            .persistent()
            .get(&pkey)
            .ok_or(Error::PoolNotFound)?;

        // Verify the token matches the pool's token to avoid sweeping from
        // wrong pools or accidental token address mistakes.
        if pool.token != token {
            return Err(Error::TokenMismatch);
        }

        // Query actual balance from the token contract.
        let token_client = token::Client::new(&env, &token);
        let contract_address = env.current_contract_address();
        let actual_balance = token_client.balance(&contract_address);

        // Compute the true surplus: anything beyond what the pool tracks as owed.
        let surplus = actual_balance.saturating_sub(pool.balance);

        // If there's a surplus, transfer it to the recipient.
        if surplus > 0 {
            token_client.transfer(&contract_address, &recipient, &surplus);
        }

        Ok(surplus)
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
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        require_admin(&env)?.require_auth();
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        Ok(())
    }

    pub fn recover_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        let recovery: Address = env
            .storage()
            .instance()
            .get(&DataKey::Recovery)
            .ok_or(Error::NotInitialized)?;
        recovery.require_auth();
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        Ok(())
    }

    pub fn set_treasury(env: Env, new_treasury: Address) -> Result<(), Error> {
        require_admin(&env)?.require_auth();
        env.storage().instance().set(&DataKey::Treasury, &new_treasury);
        Ok(())
    }
}

fn require_admin(env: &Env) -> Result<Address, Error> {
    mergefi_common::require_admin::<DataKey>(env).ok_or(Error::NotInitialized)
}

fn extend_ttl(env: &Env, key: &DataKey) {
    mergefi_common::extend_ttl(env, key);
}

/// Extends the TTL of the contract's instance storage (#38). Instance
/// storage holds Admin, Treasury, and FeeBps — losing it takes down the
/// entire contract for every pool.
fn extend_instance_ttl(env: &Env) {
    env.storage().instance().extend_ttl(100_000, 500_000);
}
