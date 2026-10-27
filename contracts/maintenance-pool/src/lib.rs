//! MergeFi Recurring Maintenance Pool Contract
//!
//! Unlike the escrow contract (funds tied to a single issue) or the
//! milestones contract (a lump sum allocated across issues in a release),
//! a maintenance pool is a standing balance tied to a repository or org.
//! Sponsors can deposit into it repeatedly over time; maintainers draw
//! down rewards for ongoing maintenance-type work as authorized by the
//! backend oracle, which tracks off-chain maintenance activity.
#![no_std]
#![warn(missing_docs)]

mod error;
mod types;

#[cfg(test)]
mod test;

use error::Error;
use soroban_sdk::{contract, contractevent, contractimpl, token, Address, BytesN, Env, Vec};
use types::{DataKey, Deposit, MaintenancePool};

use mergefi_common::{extend_instance_ttl, BPS_DENOMINATOR};

/// Inactivity window (in seconds) after which a deposit becomes
/// permissionlessly reclaimable by its original sponsor (#42). If no
/// `withdraw` occurs against the pool for this duration, any sponsor
/// can reclaim their own deposit. This mirrors escrow's GRACE_PERIOD
/// concept but applied per-deposit rather than per-pool.
pub const INACTIVITY_WINDOW: u64 = 90 * 24 * 60 * 60; // 90 days

/// Maximum number of deposits a single pool may record (issue #94).
/// `deposit` and `withdraw` refresh the TTL of every deposit sub-record in
/// a loop over `deposit_count`, so an unbounded count would make those
/// calls grow without limit, and Soroban caps a transaction footprint at
/// 100 ledger entries (a cap of 100 would already exceed it). Mirrors
/// `MAX_SPONSORS` in escrow/milestones.
pub const MAX_DEPOSITS: u32 = 50;

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
        pub recovery: Option<Address>,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct PoolDeposited {
        #[topic]
        pub pool_id: u64,
        #[topic]
        pub sponsor: Address,
        pub token: Address,
        pub amount: i128,
        pub balance: i128,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct PoolWithdrawn {
        #[topic]
        pub pool_id: u64,
        pub amount: i128,
        pub fee: i128,
        pub treasury: Address,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct PoolPaidTo {
        #[topic]
        pub pool_id: u64,
        #[topic]
        pub recipient: Address,
        pub amount: i128,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct DepositReclaimed {
        #[topic]
        pub pool_id: u64,
        #[topic]
        pub sponsor: Address,
        pub deposit_index: u32,
        pub amount: i128,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct PoolSwept {
        #[topic]
        pub pool_id: u64,
        #[topic]
        pub recipient: Address,
        pub token: Address,
        pub amount: i128,
    }

    #[contractevent]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct PoolKeptAlive {
        #[topic]
        pub pool_id: u64,
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
    pub struct ContractUpgraded {
        #[topic]
        pub admin: Address,
        pub wasm_hash: BytesN<32>,
        pub version: u32,
    }

    #[contract]
    pub struct MaintenancePoolContract;

    #[warn(missing_docs)]
    #[contractimpl]
    impl MaintenancePoolContract {
        /// One-time setup. Requires `admin`'s own authorization, so nobody can
        /// name a third-party address as admin without that address's consent
        /// — see `docs/access-control-audit.md` for what this does and does
        /// not protect against (it does not stop initializer front-running).
        pub fn initialize(
            env: Env,
            admin: Address,
            oracle: Address,
            treasury: Address,
            fee_bps: u32,
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
            env.storage().instance().set(&DataKey::Admin, &admin);
            env.storage().instance().set(&DataKey::Oracle, &oracle);
            env.storage().instance().set(&DataKey::Treasury, &treasury);
            env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
            env.storage()
                .instance()
                .set(&DataKey::Version, &CONTRACT_VERSION);
            env.storage().instance().set(&DataKey::Paused, &false);
            if let Some(r) = recovery.clone() {
                env.storage().instance().set(&DataKey::Recovery, &r);
            }
            extend_instance_ttl(&env);
            ContractInitialized {
                admin,
                oracle,
                treasury,
                fee_bps,
                recovery,
            }
            .publish(&env);
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
            if is_paused(&env) {
                return Err(Error::ContractPaused);
            }

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

            let mut existing_index = None;
            let search_limit = pool.deposit_count.min(MAX_DEPOSITS);
            for i in 0..search_limit {
                let dkey = DataKey::Deposit(pool_id, i);
                if let Some(deposit) = env.storage().persistent().get::<_, Deposit>(&dkey) {
                    if deposit.sponsor == sponsor {
                        existing_index = Some(i);
                        break;
                    }
                }
            }

            if existing_index.is_none() && pool.deposit_count >= MAX_DEPOSITS {
                return Err(Error::TooManyDeposits);
            }

            let token_client = token::Client::new(&env, &token);
            let actual_received = mergefi_common::measure_transfer_delta(
                &env,
                &token,
                &env.current_contract_address(),
                || {
                    token_client.transfer(&sponsor, env.current_contract_address(), &amount);
                },
            );

            if actual_received <= 0 {
                return Err(Error::InvalidAmount);
            }

            pool.balance = pool
                .balance
                .checked_add(actual_received)
                .ok_or(Error::ArithmeticOverflow)?;
            pool.total_deposited = pool
                .total_deposited
                .checked_add(actual_received)
                .ok_or(Error::ArithmeticOverflow)?;

            if let Some(index) = existing_index {
                let dkey = DataKey::Deposit(pool_id, index);
                let mut deposit: Deposit = env
                    .storage()
                    .persistent()
                    .get(&dkey)
                    .ok_or(Error::DepositNotFound)?;
                deposit.amount = deposit
                    .amount
                    .checked_add(actual_received)
                    .ok_or(Error::ArithmeticOverflow)?;
                deposit.timestamp = env.ledger().timestamp();
                env.storage().persistent().set(&dkey, &deposit);
                extend_ttl(&env, &dkey);
            } else {
                let index = pool.deposit_count;
                pool.deposit_count = pool
                    .deposit_count
                    .checked_add(1)
                    .ok_or(Error::DepositCountOverflow)?;

                let dkey = DataKey::Deposit(pool_id, index);
                env.storage().persistent().set(
                    &dkey,
                    &Deposit {
                        sponsor: sponsor.clone(),
                        amount: actual_received,
                        timestamp: env.ledger().timestamp(),
                    },
                );
                extend_ttl(&env, &dkey);
            }

            env.storage().persistent().set(&pkey, &pool);
            extend_ttl(&env, &pkey);

            // Refresh every prior deposit sub-record so older entries don't
            // archive while the pool stays active with recurring new deposits.
            // The newly-written record at `index` was already extended above;
            // the loop covers the full range so all historical records are refreshed.
            for i in 0..pool.deposit_count {
                extend_ttl(&env, &DataKey::Deposit(pool_id, i));
            }
            extend_instance_ttl(&env);
            PoolDeposited {
                pool_id,
                sponsor,
                token,
                amount: actual_received,
                balance: pool.balance,
            }
            .publish(&env);

            Ok(())
        }

        /// Oracle-authorized: pays `amount` out of the pool, split across
        /// `recipients` (basis-points weights summing to 10 000) after
        /// deducting the protocol fee via `mergefi_common::compute_split`
        /// — the same path escrow and milestones use (#483, #484).
        /// Rejects if the pool balance is insufficient.
        pub fn withdraw(
            env: Env,
            pool_id: u64,
            recipients: Vec<(Address, u32)>,
            amount: i128,
        ) -> Result<(), Error> {
            if is_paused(&env) {
                return Err(Error::ContractPaused);
            }

            require_oracle(&env)?.require_auth();

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
            let token_client = token::Client::new(&env, &pool.token);
            let contract_address = env.current_contract_address();

            pool.balance = pool
                .balance
                .checked_sub(amount)
                .ok_or(Error::ArithmeticOverflow)?;
            pool.total_withdrawn = pool
                .total_withdrawn
                .checked_add(amount)
                .ok_or(Error::ArithmeticOverflow)?;
            pool.last_withdraw_at = env.ledger().timestamp();
            env.storage().persistent().set(&pkey, &pool);
            extend_ttl(&env, &pkey);

            if payouts.fee > 0 {
                token_client.transfer(&contract_address, &treasury, &payouts.fee);
            }
            for (recipient, share) in payouts.shares.iter() {
                if share > 0 {
                    token_client.transfer(&contract_address, &recipient, &share);
                    PoolPaidTo {
                        pool_id,
                        recipient,
                        amount: share,
                    }
                    .publish(&env);
                }
            }

            // Refresh all deposit sub-records on every withdrawal so historical
            // deposit records stay queryable across a long-running pool's lifetime.
            for i in 0..pool.deposit_count {
                extend_ttl(&env, &DataKey::Deposit(pool_id, i));
            }
            extend_instance_ttl(&env);
            PoolWithdrawn {
                pool_id,
                amount,
                fee: payouts.fee,
                treasury,
            }
            .publish(&env);

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
        /// - The pro-rata share of the current pool balance rounds to zero
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
            // A deposit is activity too: using its timestamp prevents a new or
            // recently topped-up pool from becoming reclaimable immediately.
            let last_activity = pool
                .last_withdraw_at
                .max(pool.created_at)
                .max(deposit.timestamp);
            if now < last_activity.saturating_add(INACTIVITY_WINDOW) {
                return Err(Error::InactivityWindowNotElapsed);
            }

            // Reject a second reclaim before any other check so we never
            // transfer from a zeroed record (#479).
            if deposit.amount == 0 {
                return Err(Error::AlreadyReclaimed);
            }

            let mut total_unreclaimed = 0i128;
            for index in 0..pool.deposit_count {
                let outstanding: Deposit = env
                    .storage()
                    .persistent()
                    .get(&DataKey::Deposit(pool_id, index))
                    .ok_or(Error::DepositNotFound)?;
                total_unreclaimed = total_unreclaimed
                    .checked_add(outstanding.amount)
                    .ok_or(Error::ArithmeticOverflow)?;
            }

            let transfer_amount = pool
                .balance
                .checked_mul(deposit.amount)
                .ok_or(Error::ArithmeticOverflow)?
                / total_unreclaimed;
            if transfer_amount == 0 {
                return Err(Error::InsufficientBalance);
            }

            // Zero the deposit record **before** the transfer so a re-entrant or
            // repeated call cannot drain other sponsors' funds (#479).
            let mut zeroed = deposit.clone();
            zeroed.amount = 0;
            env.storage().persistent().set(&dkey, &zeroed);

            let token_client = token::Client::new(&env, &pool.token);
            token_client.transfer(&env.current_contract_address(), &sponsor, &transfer_amount);

            pool.balance = pool
                .balance
                .checked_sub(transfer_amount)
                .ok_or(Error::ArithmeticOverflow)?;
            // A reclaim is a capital outflow and must be tracked the same way as a
            // normal withdrawal, so that get_pool and any off-chain accounting that
            // reads total_withdrawn always reflects the full amount that left the
            // pool. Without this update, reclaimed funds appear as "still in" the
            // pool, breaking balance invariants. (#485)
            pool.total_withdrawn = pool
                .total_withdrawn
                .checked_add(transfer_amount)
                .ok_or(Error::ArithmeticOverflow)?;
            env.storage().persistent().set(&pkey, &pool);
            extend_ttl(&env, &pkey);
            extend_instance_ttl(&env);
            DepositReclaimed {
                pool_id,
                sponsor,
                deposit_index,
                amount: transfer_amount,
            }
            .publish(&env);

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

            extend_instance_ttl(&env);

            PoolKeptAlive { pool_id }.publish(&env);
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
            require_admin(&env)?.require_auth();

            let pkey = DataKey::Pool(pool_id);
            let pool: MaintenancePool = env
                .storage()
                .persistent()
                .get(&pkey)
                .ok_or(Error::PoolNotFound)?;

            // Sweeping surplus back to the contract itself is a no-op at best and
            // an accounting error at worst; reject it the same way initialize
            // rejects treasury == contract (issue #329).
            if recipient == env.current_contract_address() {
                return Err(Error::InvalidTreasury);
            }

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

            PoolSwept {
                pool_id,
                recipient,
                token,
                amount: surplus,
            }
            .publish(&env);
            Ok(surplus)
        }

        /// Pause the contract, blocking new deposits and routine withdrawals.
        /// Reclaim and sweep paths remain available for recovery.
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

        /// Returns the maintenance pool record for `pool_id`.
        pub fn get_pool(env: Env, pool_id: u64) -> Result<MaintenancePool, Error> {
            env.storage()
                .persistent()
                .get(&DataKey::Pool(pool_id))
                .ok_or(Error::PoolNotFound)
        }

        /// Returns the `index`-th deposit recorded for `pool_id`.
        pub fn get_deposit(env: Env, pool_id: u64, index: u32) -> Result<Deposit, Error> {
            env.storage()
                .persistent()
                .get(&DataKey::Deposit(pool_id, index))
                .ok_or(Error::DepositNotFound)
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

        /// Returns the fee basis points stored in instance storage.
        pub fn get_fee_bps(env: Env) -> Result<u32, Error> {
            mergefi_common::get_fee_bps::<DataKey>(&env).ok_or(Error::NotInitialized)
        }

        /// Returns the current contract version.
        pub fn get_version(env: Env) -> u32 {
            env.storage().instance().get(&DataKey::Version).unwrap_or(0)
        }

        /// Returns the basis-point denominator used for fees and payouts.
        pub fn get_bps_denominator(_env: Env) -> i128 {
            BPS_DENOMINATOR
        }

        /// Returns the maximum number of deposits recorded per pool.
        pub fn get_max_deposits(_env: Env) -> u32 {
            MAX_DEPOSITS
        }

        /// Returns the deposit inactivity window in seconds.
        pub fn get_inactivity_window(_env: Env) -> u64 {
            INACTIVITY_WINDOW
        }

        /// Admin-only: rotate the admin address. Requires both current admin
        /// and new admin authorization.
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

        /// Admin-only: rotate the oracle address. Requires both current admin
        /// and new oracle authorization.
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

        /// Recovery-authorized admin rotation: if a recovery address was provided at
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

        /// Admin-only: rotate the treasury address.
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
    }
} // mod contract

pub use contract::*;

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
