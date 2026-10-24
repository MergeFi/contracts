#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    token, vec, Address, Env, Event,
};

fn assert_event<T: Event>(env: &Env, contract_id: &Address, event: T) {
    assert!(env
        .events()
        .all()
        .events()
        .contains(&event.to_xdr(env, contract_id)));
}

fn create_token<'a>(
    env: &Env,
    admin: &Address,
) -> (Address, token::StellarAssetClient<'a>, token::Client<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let address = sac.address();
    (
        address.clone(),
        token::StellarAssetClient::new(env, &address),
        token::Client::new(env, &address),
    )
}

fn setup(env: &Env) -> (Address, Address, MaintenancePoolContractClient<'_>) {
    let admin = Address::generate(env);
    let oracle = Address::generate(env);
    let treasury = Address::generate(env);
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(env, &contract_id);
    client.initialize(&admin, &oracle, &treasury, &1_000u32, &None); // 10% fee
    (admin, treasury, client)
}

struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.state >> 32) as u32
    }

    fn next_range(&mut self, min: u32, max: u32) -> u32 {
        min + (self.next_u32() % (max - min + 1))
    }
}

#[test]
fn test_maintenance_pool_invariant_fuzzing() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let contract_id = client.address.clone();
    let pool_id = 538u64;
    let sponsors = vec![
        &env,
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    let maintainer = Address::generate(&env);
    for sponsor in sponsors.iter() {
        asset_client.mint(&sponsor, &1_000_000_000i128);
    }

    let mut rng = SimpleRng::new(538);
    for _ in 0..300 {
        env.ledger().set_timestamp(
            env.ledger()
                .timestamp()
                .saturating_add(INACTIVITY_WINDOW + 1),
        );

        match rng.next_range(0, 2) {
            0 => {
                let sponsor = sponsors.get(rng.next_range(0, 3)).unwrap();
                let amount = rng.next_range(1, 100_000) as i128;
                let _ = client.try_deposit(&pool_id, &sponsor, &token_addr, &amount);
            }
            1 => {
                let recipients = vec![&env, (maintainer.clone(), 10_000u32)];
                let amount = rng.next_range(1, 100_000) as i128;
                let _ = client.try_withdraw(&pool_id, &recipients, &amount);
            }
            2 => {
                if let Ok(Ok(pool)) = client.try_get_pool(&pool_id) {
                    if pool.deposit_count > 0 {
                        let index = rng.next_range(0, pool.deposit_count - 1);
                        let sponsor = client.get_deposit(&pool_id, &index).sponsor;
                        let _ = client.try_reclaim_deposit(&pool_id, &index, &sponsor);
                    }
                }
            }
            _ => unreachable!(),
        }

        if let Ok(Ok(pool)) = client.try_get_pool(&pool_id) {
            assert_eq!(
                pool.balance,
                pool.total_deposited - pool.total_withdrawn,
                "pool books diverged after a randomized operation"
            );
            assert_eq!(
                token_client.balance(&contract_id),
                pool.balance,
                "pool book balance diverged from token balance"
            );
        }
    }
}

#[test]
fn test_pool_lifecycle_and_admin_events() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let recovery = Address::generate(&env);
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(&env, &contract_id);
    client.initialize(
        &admin,
        &oracle,
        &treasury,
        &1_000u32,
        &Some(recovery.clone()),
    );
    assert_event(
        &env,
        &contract_id,
        ContractInitialized {
            admin: admin.clone(),
            oracle: oracle.clone(),
            treasury: treasury.clone(),
            fee_bps: 1_000,
            recovery: Some(recovery.clone()),
        },
    );

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor_a = Address::generate(&env);
    let sponsor_b = Address::generate(&env);
    asset_client.mint(&sponsor_a, &2_000i128);
    asset_client.mint(&sponsor_b, &1_000i128);

    client.deposit(&445u64, &sponsor_a, &token_addr, &1_000i128);
    assert_event(
        &env,
        &contract_id,
        PoolDeposited {
            pool_id: 445,
            sponsor: sponsor_a.clone(),
            token: token_addr.clone(),
            amount: 1_000,
            balance: 1_000,
        },
    );
    client.deposit(&445u64, &sponsor_b, &token_addr, &1_000i128);

    let recipient = Address::generate(&env);
    client.withdraw(
        &445u64,
        &vec![&env, (recipient.clone(), 10_000u32)],
        &1_000i128,
    );
    assert_event(
        &env,
        &contract_id,
        PoolPaidTo {
            pool_id: 445,
            recipient,
            amount: 900,
        },
    );
    assert_event(
        &env,
        &contract_id,
        PoolWithdrawn {
            pool_id: 445,
            amount: 1_000,
            fee: 100,
            treasury: treasury.clone(),
        },
    );

    env.ledger().set_timestamp(INACTIVITY_WINDOW);
    client.reclaim_deposit(&445u64, &0u32, &sponsor_a);
    assert_event(
        &env,
        &contract_id,
        DepositReclaimed {
            pool_id: 445,
            sponsor: sponsor_a.clone(),
            deposit_index: 0,
            amount: 500,
        },
    );
    token_client.transfer(&sponsor_a, &contract_id, &17i128);
    assert_eq!(client.sweep(&445u64, &token_addr, &sponsor_b), 17);
    assert_event(
        &env,
        &contract_id,
        PoolSwept {
            pool_id: 445,
            recipient: sponsor_b.clone(),
            token: token_addr.clone(),
            amount: 17,
        },
    );
    client.keep_alive(&445u64);
    assert_event(&env, &contract_id, PoolKeptAlive { pool_id: 445 });

    let new_admin = Address::generate(&env);
    client.set_admin(&new_admin);
    assert_event(
        &env,
        &contract_id,
        AdminChanged {
            old_admin: admin,
            new_admin: new_admin.clone(),
        },
    );
    let new_oracle = Address::generate(&env);
    client.set_oracle(&new_oracle);
    assert_event(
        &env,
        &contract_id,
        OracleChanged {
            old_oracle: oracle,
            new_oracle,
        },
    );
    let recovered_admin = Address::generate(&env);
    client.recover_admin(&recovered_admin);
    assert_event(
        &env,
        &contract_id,
        AdminRecovered {
            recovery,
            new_admin: recovered_admin.clone(),
        },
    );
    let new_treasury = Address::generate(&env);
    client.set_treasury(&new_treasury);
    assert_event(
        &env,
        &contract_id,
        TreasuryChanged {
            old_treasury: treasury,
            new_treasury,
        },
    );
    client.pause();
    assert_event(
        &env,
        &contract_id,
        ContractPaused {
            admin: recovered_admin.clone(),
        },
    );
    client.unpause();
    assert_event(
        &env,
        &contract_id,
        ContractUnpaused {
            admin: recovered_admin.clone(),
        },
    );

    let wasm_hash = native_wasm_hash(&env);
    client.upgrade(&wasm_hash);
    assert_event(
        &env,
        &contract_id,
        ContractUpgraded {
            admin: recovered_admin,
            wasm_hash,
            version: CONTRACT_VERSION,
        },
    );
}

#[test]
fn test_get_admin_treasury_fee_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, treasury, client) = setup(&env);

    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_treasury(), treasury);
    assert_eq!(client.get_fee_bps(), 1_000u32);
    assert_eq!(client.get_version(), CONTRACT_VERSION);
    assert_eq!(client.get_bps_denominator(), BPS_DENOMINATOR);
    assert_eq!(client.get_max_deposits(), MAX_DEPOSITS);
    assert_eq!(client.get_inactivity_window(), INACTIVITY_WINDOW);
}

#[test]
fn test_get_admin_treasury_fee_bps_before_initialize() {
    let env = Env::default();
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(&env, &contract_id);

    assert_eq!(client.try_get_admin(), Err(Ok(Error::NotInitialized)));
    assert_eq!(client.try_get_treasury(), Err(Ok(Error::NotInitialized)));
    assert_eq!(client.try_get_fee_bps(), Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_get_pool_and_withdraw_reject_nonexistent_pool() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);
    let maintainer = Address::generate(&env);

    let get_err = client.try_get_pool(&404u64);
    assert_eq!(get_err, Err(Ok(Error::PoolNotFound)));

    let recipients = vec![&env, (maintainer.clone(), 10_000u32)];
    let withdraw_err = client.try_withdraw(&404u64, &recipients, &1i128);
    assert_eq!(withdraw_err, Err(Ok(Error::PoolNotFound)));
}

#[test]
fn test_deposit_accumulates_balance_and_history() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor_a = Address::generate(&env);
    let sponsor_b = Address::generate(&env);
    asset_client.mint(&sponsor_a, &500_0000000i128);
    asset_client.mint(&sponsor_b, &300_0000000i128);

    client.deposit(&1u64, &sponsor_a, &token_addr, &500_0000000i128);
    client.deposit(&1u64, &sponsor_b, &token_addr, &300_0000000i128);

    let pool = client.get_pool(&1u64);
    assert_eq!(pool.balance, 800_0000000i128);
    assert_eq!(pool.total_deposited, 800_0000000i128);
    assert_eq!(pool.deposit_count, 2);

    let d0 = client.get_deposit(&1u64, &0u32);
    assert_eq!(d0.sponsor, sponsor_a);
    assert_eq!(d0.amount, 500_0000000i128);
    let d1 = client.get_deposit(&1u64, &1u32);
    assert_eq!(d1.sponsor, sponsor_b);
    assert_eq!(d1.amount, 300_0000000i128);
}

#[test]
fn test_withdraw_deducts_fee_and_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);
    client.deposit(&2u64, &sponsor, &token_addr, &10_000_000_000i128);

    let maintainer = Address::generate(&env);
    client.withdraw(&2u64, &vec![&env, (maintainer.clone(), 10_000u32)], &200_0000000i128);

    // 10% fee -> 20_0000000 to treasury, 180_0000000 to maintainer.
    assert_eq!(token_client.balance(&maintainer), 180_0000000i128);
    assert_eq!(token_client.balance(&treasury), 20_0000000i128);

    let pool = client.get_pool(&2u64);
    assert_eq!(pool.balance, 800_0000000i128);
    assert_eq!(pool.total_withdrawn, 200_0000000i128);
}

#[test]
fn test_withdraw_with_zero_fee_pays_full_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(&env, &contract_id);
    client.initialize(&admin, &oracle, &treasury, &0u32, &None);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);
    client.deposit(&2u64, &sponsor, &token_addr, &10_000_000_000i128);

    let maintainer = Address::generate(&env);
    client.withdraw(&2u64, &vec![&env, (maintainer.clone(), 10_000u32)], &200_0000000i128);

    assert_eq!(token_client.balance(&maintainer), 200_0000000i128);
    assert_eq!(token_client.balance(&treasury), 0i128);

    let pool = client.get_pool(&2u64);
    assert_eq!(pool.balance, 800_0000000i128);
    assert_eq!(pool.total_withdrawn, 200_0000000i128);
}

#[test]
fn test_deposit_rejects_invalid_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, _asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);

    let err = client.try_deposit(&4u64, &sponsor, &token_addr, &0i128);
    assert_eq!(err, Err(Ok(Error::InvalidAmount)));
    let err = client.try_deposit(&4u64, &sponsor, &token_addr, &-1i128);
    assert_eq!(err, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn test_withdraw_rejects_invalid_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &100_0000000i128);
    client.deposit(&5u64, &sponsor, &token_addr, &100_0000000i128);

    let maintainer = Address::generate(&env);
    let recipients = vec![&env, (maintainer.clone(), 10_000u32)];
    let err = client.try_withdraw(&5u64, &recipients, &0i128);
    assert_eq!(err, Err(Ok(Error::InvalidAmount)));
    let err = client.try_withdraw(&5u64, &recipients, &-1i128);
    assert_eq!(err, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn test_withdraw_rejects_insufficient_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &100_0000000i128);
    client.deposit(&3u64, &sponsor, &token_addr, &100_0000000i128);

    let maintainer = Address::generate(&env);
    let recipients = vec![&env, (maintainer.clone(), 10_000u32)];
    let err = client.try_withdraw(&3u64, &recipients, &200_0000000i128);
    assert_eq!(err, Err(Ok(Error::InsufficientBalance)));
}

#[test]
fn test_deposit_rejects_token_mismatch() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _t1) = create_token(&env, &token_admin);
    let (other_token_addr, other_asset_client, _t2) = create_token(&env, &token_admin);

    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &100_0000000i128);
    other_asset_client.mint(&sponsor, &100_0000000i128);

    client.deposit(&4u64, &sponsor, &token_addr, &50_0000000i128);
    let err = client.try_deposit(&4u64, &sponsor, &other_token_addr, &50_0000000i128);
    assert_eq!(err, Err(Ok(Error::TokenMismatch)));
}

// ---------------------------------------------------------------------------
// Access-control boundary matrix (#30)
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_requires_admin_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(&env, &contract_id);

    let result = client.try_initialize(&admin, &oracle, &treasury, &1_000u32, &None);
    assert!(result.is_err());
}

#[test]
fn test_deposit_requires_sponsor_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &100_0000000i128);

    env.set_auths(&[]);
    let result = client.try_deposit(&5u64, &sponsor, &token_addr, &50_0000000i128);
    assert!(result.is_err());
}

#[test]
fn test_withdraw_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &100_0000000i128);
    client.deposit(&6u64, &sponsor, &token_addr, &100_0000000i128);

    env.set_auths(&[]);
    let maintainer = Address::generate(&env);
    let recipients = vec![&env, (maintainer.clone(), 10_000u32)];
    let result = client.try_withdraw(&6u64, &recipients, &50_0000000i128);
    assert!(result.is_err());
}

#[test]
fn test_initialize_rejects_double_init() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(&env, &contract_id);

    // First initialization succeeds
    client.initialize(&admin, &oracle, &treasury, &1_000u32, &None);

    // Second initialization should fail with AlreadyInitialized
    let new_oracle = Address::generate(&env);
    let result = client.try_initialize(&admin, &new_oracle, &treasury, &1_000u32, &None);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_initialize_rejects_invalid_fee() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(&env, &contract_id);

    // fee_bps > 10000 should fail with InvalidFee
    let result = client.try_initialize(&admin, &oracle, &treasury, &10_001u32, &None);
    assert_eq!(result, Err(Ok(Error::InvalidFee)));
}

// ---------------------------------------------------------------------------
// Withdrawal boundary, ledger consistency, and multi-sponsor history (#56/#11)
// ---------------------------------------------------------------------------

#[test]
fn test_withdraw_exact_balance_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &500_0000000i128);
    client.deposit(&7u64, &sponsor, &token_addr, &500_0000000i128);

    let maintainer = Address::generate(&env);
    let recipients = vec![&env, (maintainer.clone(), 10_000u32)];

    // One more than the exact balance must be rejected.
    let err = client.try_withdraw(&7u64, &recipients, &500_0000001i128);
    assert_eq!(err, Err(Ok(Error::InsufficientBalance)));

    // Exactly the pool's balance must succeed and drain it to zero.
    client.withdraw(&7u64, &recipients, &500_0000000i128);
    let pool = client.get_pool(&7u64);
    assert_eq!(pool.balance, 0i128);
    assert_eq!(pool.total_withdrawn, 500_0000000i128);
}

#[test]
fn test_interleaved_deposit_withdraw_consistency() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    let maintainer = Address::generate(&env);
    asset_client.mint(&sponsor, &18_000_000_000i128);

    let mut total_deposited = 0i128;
    let mut total_withdrawn = 0i128;

    for (deposit_amt, withdraw_amt) in [
        (10_000_000_000i128, 2_000_000_000i128),
        (5_000_000_000i128, 1_000_000_000i128),
        (3_000_000_000i128, 0i128),
    ] {
        client.deposit(&8u64, &sponsor, &token_addr, &deposit_amt);
        total_deposited += deposit_amt;

        let pool = client.get_pool(&8u64);
        assert_eq!(pool.total_deposited, total_deposited);
        assert_eq!(pool.balance, total_deposited - total_withdrawn);

        if withdraw_amt > 0 {
            client.withdraw(&8u64, &vec![&env, (maintainer.clone(), 10_000u32)], &withdraw_amt);
            total_withdrawn += withdraw_amt;

            let pool = client.get_pool(&8u64);
            assert_eq!(pool.total_withdrawn, total_withdrawn);
            assert_eq!(pool.balance, total_deposited - total_withdrawn);
        }
    }

    let pool = client.get_pool(&8u64);
    assert_eq!(pool.total_deposited, 18_000_000_000i128);
    assert_eq!(pool.total_withdrawn, 3_000_000_000i128);
    assert_eq!(pool.balance, 15_000_000_000i128);
}

#[test]
fn test_multiple_sponsors_deposit_history() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor_a = Address::generate(&env);
    let sponsor_b = Address::generate(&env);
    let sponsor_c = Address::generate(&env);
    asset_client.mint(&sponsor_a, &100_0000000i128);
    asset_client.mint(&sponsor_b, &200_0000000i128);
    asset_client.mint(&sponsor_c, &300_0000000i128);

    client.deposit(&9u64, &sponsor_a, &token_addr, &100_0000000i128);
    client.deposit(&9u64, &sponsor_b, &token_addr, &200_0000000i128);
    client.deposit(&9u64, &sponsor_c, &token_addr, &300_0000000i128);

    let pool = client.get_pool(&9u64);
    assert_eq!(pool.deposit_count, 3);
    assert_eq!(pool.total_deposited, 600_0000000i128);
    assert_eq!(pool.balance, 600_0000000i128);

    let expected = [
        (sponsor_a, 100_0000000i128),
        (sponsor_b, 200_0000000i128),
        (sponsor_c, 300_0000000i128),
    ];
    for (index, (sponsor, amount)) in expected.into_iter().enumerate() {
        let deposit = client.get_deposit(&9u64, &(index as u32));
        assert_eq!(deposit.sponsor, sponsor);
        assert_eq!(deposit.amount, amount);
    }
}

#[test]
fn test_direct_transfer_creates_unrecoverable_surplus_before_sweep() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    let _maintainer = Address::generate(&env);
    let contract_addr = client.address.clone();

    // Mint tokens to sponsor
    asset_client.mint(&sponsor, &1000_0000000i128);

    // Deposit via the normal deposit function
    client.deposit(&1u64, &sponsor, &token_addr, &500_0000000i128);

    // Directly transfer 200 tokens to the contract (simulating direct transfer)
    token_client.transfer(&sponsor, &contract_addr, &200_0000000i128);

    // Pool balance is still 500, not accounting for the direct transfer
    let pool = client.get_pool(&1u64);
    assert_eq!(pool.balance, 500_0000000i128);

    // Contract's actual token balance should be 700
    let contract_balance = token_client.balance(&contract_addr);
    assert_eq!(contract_balance, 700_0000000i128);

    // Before sweep, there's no way to recover the surplus 200
    // Now the sweep should be able to recover it
}

#[test]
fn test_sweep_recovers_surplus_after_direct_transfer() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    let contract_addr = client.address.clone();

    // Mint tokens to sponsor
    asset_client.mint(&sponsor, &1000_0000000i128);

    // Deposit via the normal deposit function
    client.deposit(&1u64, &sponsor, &token_addr, &500_0000000i128);

    // Directly transfer 200 tokens to the contract (simulating stray transfer)
    token_client.transfer(&sponsor, &contract_addr, &200_0000000i128);

    // Verify before state
    let pool_before = client.get_pool(&1u64);
    assert_eq!(pool_before.balance, 500_0000000i128);
    let contract_balance_before = token_client.balance(&contract_addr);
    assert_eq!(contract_balance_before, 700_0000000i128);
    let treasury_balance_before = token_client.balance(&treasury);

    // Execute sweep
    let swept_amount = client.sweep(&1u64, &token_addr, &treasury);
    assert_eq!(swept_amount, 200_0000000i128);

    // Verify after state: pool balance unchanged
    let pool_after = client.get_pool(&1u64);
    assert_eq!(pool_after.balance, 500_0000000i128);

    // Contract balance should be reduced by swept amount
    let contract_balance_after = token_client.balance(&contract_addr);
    assert_eq!(contract_balance_after, 500_0000000i128);

    // Treasury should have received the swept amount
    let treasury_balance_after = token_client.balance(&treasury);
    assert_eq!(
        treasury_balance_after,
        treasury_balance_before + 200_0000000i128
    );
}

#[test]
fn test_sweep_cannot_remove_owed_balances() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    let maintainer = Address::generate(&env);
    let contract_addr = client.address.clone();

    // Mint tokens to sponsor
    asset_client.mint(&sponsor, &1000_0000000i128);

    // Deposit 500 tokens into pool
    client.deposit(&1u64, &sponsor, &token_addr, &500_0000000i128);

    // Add extra 200 via direct transfer (surplus)
    token_client.transfer(&sponsor, &contract_addr, &200_0000000i128);

    // Verify state before sweep
    let pool = client.get_pool(&1u64);
    assert_eq!(pool.balance, 500_0000000i128);
    let contract_balance = token_client.balance(&contract_addr);
    assert_eq!(contract_balance, 700_0000000i128);

    // Sweep should only remove the 200 surplus
    let swept_amount = client.sweep(&1u64, &token_addr, &treasury);
    assert_eq!(swept_amount, 200_0000000i128);

    // Pool balance should still be intact (500)
    let pool_after = client.get_pool(&1u64);
    assert_eq!(pool_after.balance, 500_0000000i128);

    // Withdrawal should still work for the full pool balance
    let maintainer_balance_before = token_client.balance(&maintainer);
    client.withdraw(&1u64, &vec![&env, (maintainer.clone(), 10_000u32)], &100_0000000i128); // 10% fee = 10, payout = 90
    let maintainer_balance_after = token_client.balance(&maintainer);

    // Maintainer should receive (100 - 10% fee) = 90
    let expected_payout = 90_0000000i128;
    assert_eq!(
        maintainer_balance_after,
        maintainer_balance_before + expected_payout
    );

    // Pool balance should be reduced by withdrawn amount
    let pool_final = client.get_pool(&1u64);
    assert_eq!(pool_final.balance, 400_0000000i128);
}

#[test]
fn test_sweep_with_zero_surplus_returns_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);

    // Mint and deposit exactly
    asset_client.mint(&sponsor, &500_0000000i128);
    client.deposit(&1u64, &sponsor, &token_addr, &500_0000000i128);

    // Sweep should find no surplus (actual balance == pool balance)
    let swept_amount = client.sweep(&1u64, &token_addr, &treasury);
    assert_eq!(swept_amount, 0i128);

    // Pool state should be unchanged
    let pool = client.get_pool(&1u64);
    assert_eq!(pool.balance, 500_0000000i128);
}

#[test]
fn test_sweep_only_admin_authorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &500_0000000i128);

    client.deposit(&1u64, &sponsor, &token_addr, &500_0000000i128);

    // Try to sweep as non-admin should fail
    env.set_auths(&[]);
    let sweep_err = client.try_sweep(&1u64, &token_addr, &treasury);
    assert!(sweep_err.is_err());
}

#[test]
fn test_sweep_rejects_mismatched_token() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let (wrong_token_addr, _wrong_asset_client, _wrong_token_client) =
        create_token(&env, &token_admin);

    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &500_0000000i128);

    // Deposit with token_addr
    client.deposit(&1u64, &sponsor, &token_addr, &500_0000000i128);

    // Try to sweep with wrong token should fail
    let sweep_err = client.try_sweep(&1u64, &wrong_token_addr, &treasury);
    assert_eq!(sweep_err, Err(Ok(Error::TokenMismatch)));
}

#[test]
fn test_recover_withdraw_frozen_before_recoverable_after() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let recovery = Address::generate(&env);
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(&env, &contract_id);

    // Initialize with a recovery address
    env.mock_all_auths();
    client.initialize(
        &admin,
        &oracle,
        &treasury,
        &1_000u32,
        &Some(recovery.clone()),
    );

    // Deposit into pool
    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);
    client.deposit(&42u64, &sponsor, &token_addr, &10_000_000_000i128);

    // Simulate lost admin: clear auths
    env.set_auths(&[]);
    let maintainer = Address::generate(&env);
    let recipients = vec![&env, (maintainer.clone(), 10_000u32)];
    let err = client.try_withdraw(&42u64, &recipients, &1_000_000_000i128);
    assert!(err.is_err());

    // Recovery installs a new admin via the contract entrypoint.
    env.mock_all_auths();
    let new_admin = Address::generate(&env);
    client.recover_admin(&new_admin);
    // New admin withdraws successfully (mocked auth enables it)
    env.mock_all_auths();
    client.withdraw(&42u64, &recipients, &1_000_000_000i128);
    assert_eq!(token_client.balance(&maintainer), 900_000_000i128); // after 10% fee
}

#[test]
fn test_pause_blocks_deposit_and_withdraw_but_allows_reclaim_after_inactivity() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    let maintainer = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    client.deposit(&77u64, &sponsor, &token_addr, &10_000_000_000i128);
    client.pause();
    assert!(client.is_paused_view());

    let deposit_err = client.try_deposit(&78u64, &sponsor, &token_addr, &1_000_000_000i128);
    assert_eq!(deposit_err, Err(Ok(Error::ContractPaused)));

    let recipients = vec![&env, (maintainer.clone(), 10_000u32)];
    let withdraw_err = client.try_withdraw(&77u64, &recipients, &1_000_000_000i128);
    assert_eq!(withdraw_err, Err(Ok(Error::ContractPaused)));

    env.ledger().set_timestamp(INACTIVITY_WINDOW + 1);
    client.reclaim_deposit(&77u64, &0u32, &sponsor);
    assert_eq!(token_client.balance(&sponsor), 10_000_000_000i128);
}

/// #346: `reclaim_deposit` compares with strict `<`
/// (`now < pool.last_withdraw_at + INACTIVITY_WINDOW`), so the window is
/// intended to be *inclusive* of its own boundary — a reclaim attempted at
/// exactly `last_withdraw_at + INACTIVITY_WINDOW` must succeed, not just one
/// second past it. This pins down that exact edge, which no prior test
/// exercised (the only success-path test used `INACTIVITY_WINDOW + 1`).
#[test]
fn test_reclaim_deposit_succeeds_exactly_at_inactivity_window_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &5_000_000_000i128);

    // Pool created at timestamp 0, so last_withdraw_at is initialized to 0
    // (no withdrawal has ever occurred).
    client.deposit(&91u64, &sponsor, &token_addr, &5_000_000_000i128);
    assert_eq!(client.get_pool(&91u64).last_withdraw_at, 0);

    // Exactly at the boundary: now == last_withdraw_at + INACTIVITY_WINDOW.
    env.ledger().set_timestamp(INACTIVITY_WINDOW);
    client.reclaim_deposit(&91u64, &0u32, &sponsor);
    assert_eq!(token_client.balance(&sponsor), 5_000_000_000i128);
}

/// Complements the exact-boundary success test above: one second before the
/// boundary, the window has not yet elapsed and the call must still reject.
#[test]
fn test_reclaim_deposit_rejects_one_second_before_inactivity_window_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &5_000_000_000i128);

    client.deposit(&92u64, &sponsor, &token_addr, &5_000_000_000i128);

    env.ledger().set_timestamp(INACTIVITY_WINDOW - 1);
    let err = client.try_reclaim_deposit(&92u64, &0u32, &sponsor);
    assert_eq!(err, Err(Ok(Error::InactivityWindowNotElapsed)));
}

#[test]
fn test_reclaim_deposit_refunds_pro_rata_after_shared_withdrawals() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor_a = Address::generate(&env);
    let sponsor_b = Address::generate(&env);
    asset_client.mint(&sponsor_a, &100_0000000i128);
    asset_client.mint(&sponsor_b, &100_0000000i128);

    client.deposit(&93u64, &sponsor_a, &token_addr, &100_0000000i128);
    client.deposit(&93u64, &sponsor_b, &token_addr, &100_0000000i128);

    let maintainer = Address::generate(&env);
    client.withdraw(
        &93u64,
        &vec![&env, (maintainer, 10_000u32)],
        &150_0000000i128,
    );
    assert_eq!(client.get_pool(&93u64).balance, 50_0000000i128);

    env.ledger().set_timestamp(INACTIVITY_WINDOW + 1);
    client.reclaim_deposit(&93u64, &0u32, &sponsor_a);
    assert_eq!(token_client.balance(&sponsor_a), 25_0000000i128);
    assert_eq!(client.get_pool(&93u64).balance, 25_0000000i128);

    client.reclaim_deposit(&93u64, &1u32, &sponsor_b);
    assert_eq!(token_client.balance(&sponsor_b), 25_0000000i128);
    assert_eq!(client.get_pool(&93u64).balance, 0);
}

#[test]
fn test_unpause_restores_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &2_000_000_000i128);

    client.pause();
    client.unpause();
    assert!(!client.is_paused_view());

    client.deposit(&88u64, &sponsor, &token_addr, &2_000_000_000i128);
    assert_eq!(client.get_pool(&88u64).balance, 2_000_000_000i128);
}

#[test]
fn test_deposit_rejects_when_deposit_count_would_overflow() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &1_000i128);

    // Direct storage manipulation: set deposit_count to u32::MAX
    env.as_contract(&client.address, || {
        let pkey = DataKey::Pool(10u64);
        let pool = MaintenancePool {
            token: token_addr.clone(),
            balance: 0,
            total_deposited: 0,
            total_withdrawn: 0,
            created_at: env.ledger().timestamp(),
            deposit_count: u32::MAX,
            last_withdraw_at: 0,
        };
        env.storage().persistent().set(&pkey, &pool);
    });

    // The MAX_DEPOSITS cap rejects long before u32 overflow is reachable.
    let err = client.try_deposit(&10u64, &sponsor, &token_addr, &100i128);
    assert_eq!(err, Err(Ok(Error::TooManyDeposits)));
}

#[test]
fn test_deposit_rejects_beyond_max_deposits() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);

    for _ in 0..crate::MAX_DEPOSITS {
        let sponsor = Address::generate(&env);
        asset_client.mint(&sponsor, &1i128);
        client.deposit(&11u64, &sponsor, &token_addr, &1i128);
    }
    assert_eq!(client.get_pool(&11u64).deposit_count, crate::MAX_DEPOSITS);

    let one_too_many = Address::generate(&env);
    asset_client.mint(&one_too_many, &1i128);
    let err = client.try_deposit(&11u64, &one_too_many, &token_addr, &1i128);
    assert_eq!(err, Err(Ok(Error::TooManyDeposits)));
    assert_eq!(client.get_pool(&11u64).deposit_count, crate::MAX_DEPOSITS);

    // Other pools are unaffected.
    client.deposit(&12u64, &one_too_many, &token_addr, &1i128);
}

#[test]
fn test_existing_sponsor_can_top_up_when_at_max_deposits_cap() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);

    let first_sponsor = Address::generate(&env);
    asset_client.mint(&first_sponsor, &1_000i128);
    client.deposit(&15u64, &first_sponsor, &token_addr, &100i128);

    for _ in 1..crate::MAX_DEPOSITS {
        let extra = Address::generate(&env);
        asset_client.mint(&extra, &10i128);
        client.deposit(&15u64, &extra, &token_addr, &10i128);
    }
    assert_eq!(client.get_pool(&15u64).deposit_count, crate::MAX_DEPOSITS);

    // Existing sponsor topping up must succeed and not increase deposit_count
    client.deposit(&15u64, &first_sponsor, &token_addr, &200i128);
    assert_eq!(client.get_pool(&15u64).deposit_count, crate::MAX_DEPOSITS);
    assert_eq!(client.get_deposit(&15u64, &0u32).amount, 300i128);
    assert_eq!(client.get_pool(&15u64).balance, 100i128 + 49 * 10i128 + 200i128);
}

#[test]
fn test_deposit_aggregates_multiple_deposits_from_same_sponsor() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &1_000i128);

    client.deposit(&16u64, &sponsor, &token_addr, &100i128);
    let pool = client.get_pool(&16u64);
    assert_eq!(pool.deposit_count, 1);
    assert_eq!(pool.balance, 100i128);
    assert_eq!(pool.total_deposited, 100i128);

    client.deposit(&16u64, &sponsor, &token_addr, &250i128);
    let pool = client.get_pool(&16u64);
    assert_eq!(pool.deposit_count, 1);
    assert_eq!(pool.balance, 350i128);
    assert_eq!(pool.total_deposited, 350i128);

    let deposit = client.get_deposit(&16u64, &0u32);
    assert_eq!(deposit.amount, 350i128);
    assert_eq!(deposit.sponsor, sponsor);
}

// ─── upgrade (issue #246) ────────────────────────────────────────────────────

/// Hash of the empty Wasm the test host installs as the executable for every
/// natively-registered contract. Upgrading to it keeps the native
/// implementation dispatchable, so post-upgrade behaviour can be asserted
/// without shipping a compiled .wasm fixture.
fn native_wasm_hash(env: &Env) -> BytesN<32> {
    env.crypto().sha256(&soroban_sdk::Bytes::new(env)).into()
}

#[test]
fn test_upgrade_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);
    let hash = native_wasm_hash(&env);

    // No auths at all: the admin's require_auth must fail.
    env.set_auths(&[]);
    assert!(client.try_upgrade(&hash).is_err());

    // A non-admin signing the call is rejected too.
    let attacker = Address::generate(&env);
    let result = client
        .mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &attacker,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &client.address,
                fn_name: "upgrade",
                args: soroban_sdk::IntoVal::into_val(&(hash.clone(),), &env),
                sub_invokes: &[],
            },
        }])
        .try_upgrade(&hash);
    assert!(result.is_err());
}

#[test]
fn test_upgrade_requires_explicit_idempotent_migration() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    // Simulate an initialized v1 instance before upgrading to this code.
    env.as_contract(&client.address, || {
        env.storage().instance().set(&DataKey::Version, &1u32);
    });
    assert_eq!(client.get_version(), 1);

    client.upgrade(&native_wasm_hash(&env));
    assert_eq!(client.get_version(), 1);
    client.migrate(&1u32);
    assert_eq!(client.get_version(), CONTRACT_VERSION);
    client.migrate(&1u32);
    assert_eq!(client.get_version(), CONTRACT_VERSION);
}

#[test]
fn test_migrate_requires_admin_and_matching_source_version() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    // Simulate a mid-upgrade state: stored version is 1 (post-upgrade WASM
    // hasn't called migrate yet). This lets us test the version-mismatch
    // guard without hitting the idempotent "already at CONTRACT_VERSION" path.
    env.as_contract(&client.address, || {
        env.storage().instance().set(&DataKey::Version, &1u32);
    });

    // No auth: admin's require_auth must fire.
    env.set_auths(&[]);
    assert!(client.try_migrate(&1u32).is_err());

    // Wrong from_version (0 ≠ stored 1): must be rejected.
    env.mock_all_auths();
    assert_eq!(
        client.try_migrate(&0u32),
        Err(Ok(Error::InvalidVersion))
    );
}

#[test]
fn test_upgrade_rejects_unknown_wasm_hash() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    // A hash that was never uploaded must make the host reject the upgrade.
    let bogus = BytesN::from_array(&env, &[7u8; 32]);
    assert!(client.try_upgrade(&bogus).is_err());
    assert_eq!(client.get_version(), CONTRACT_VERSION);
}

/// Maintenance pools are long-lived, so an upgrade must not disturb a pool's
/// accumulated state: every Deposit sub-record stays readable and the pool
/// keeps accepting deposits and withdrawals afterwards.
#[test]
fn test_upgrade_preserves_pool_with_multiple_deposits() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, treasury, client) = setup(&env); // 10% fee

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsors = [
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    let amounts = [1_000i128, 2_000i128, 3_000i128];
    for (sponsor, amount) in sponsors.iter().zip(amounts) {
        asset_client.mint(sponsor, &amount);
        client.deposit(&246u64, sponsor, &token_addr, &amount);
    }

    let pool_before = client.get_pool(&246u64);
    assert_eq!(pool_before.deposit_count, 3);
    assert_eq!(pool_before.balance, 6_000i128);

    client.upgrade(&native_wasm_hash(&env));

    // Pool and every Deposit record are unchanged.
    assert_eq!(client.get_pool(&246u64), pool_before);
    for (i, (sponsor, amount)) in sponsors.iter().zip(amounts).enumerate() {
        let d = client.get_deposit(&246u64, &(i as u32));
        assert_eq!(&d.sponsor, sponsor);
        assert_eq!(d.amount, amount);
    }

    // And the pool is still fully functional.
    let late_sponsor = Address::generate(&env);
    asset_client.mint(&late_sponsor, &500i128);
    client.deposit(&246u64, &late_sponsor, &token_addr, &500i128);
    assert_eq!(client.get_pool(&246u64).deposit_count, 4);
    assert_eq!(client.get_deposit(&246u64, &3u32).sponsor, late_sponsor);

    let maintainer = Address::generate(&env);
    client.withdraw(&246u64, &vec![&env, (maintainer.clone(), 10_000u32)], &1_000i128);
    let pool_after = client.get_pool(&246u64);
    assert_eq!(pool_after.balance, 5_500i128);
    assert_eq!(pool_after.total_withdrawn, 1_000i128);
    assert_eq!(
        token_client.balance(&maintainer) + token_client.balance(&treasury),
        1_000i128
    );
}

#[test]
fn test_set_oracle_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    env.set_auths(&[]);
    let new_oracle = Address::generate(&env);
    let result = client.try_set_oracle(&new_oracle);
    assert!(result.is_err());
}

#[test]
fn test_set_oracle_rotates_oracle_used_by_withdraw() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let old_oracle = client.get_oracle();
    let new_oracle = Address::generate(&env);
    client.set_oracle(&new_oracle);
    assert_eq!(client.get_oracle(), new_oracle);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &100_0000000i128);
    client.deposit(&7u64, &sponsor, &token_addr, &100_0000000i128);

    let maintainer = Address::generate(&env);
    client.withdraw(&7u64, &vec![&env, (maintainer.clone(), 10_000u32)], &50_0000000i128);

    // withdraw now requires the rotated oracle's authorization, not the old one's.
    let auths = env.auths();
    assert!(auths.iter().any(|(addr, _)| *addr == new_oracle));
    assert!(!auths.iter().any(|(addr, _)| *addr == old_oracle));
}

#[test]
fn test_set_treasury_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, treasury, client) = setup(&env);

    env.set_auths(&[]);
    let new_treasury = Address::generate(&env);
    let result = client.try_set_treasury(&new_treasury);
    assert!(result.is_err());
    assert_eq!(client.get_treasury(), treasury);
}

#[test]
fn test_set_treasury_updates_fee_recipient() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, old_treasury, client) = setup(&env);

    let new_treasury = Address::generate(&env);
    client.set_treasury(&new_treasury);
    assert_eq!(client.get_treasury(), new_treasury);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);
    client.deposit(&253u64, &sponsor, &token_addr, &10_000_000_000i128);

    let maintainer = Address::generate(&env);
    client.withdraw(&253u64, &vec![&env, (maintainer.clone(), 10_000u32)], &200_0000000i128);

    // 10% fee lands on the new treasury, none on the old one.
    assert_eq!(token_client.balance(&new_treasury), 20_0000000i128);
    assert_eq!(token_client.balance(&old_treasury), 0i128);
    assert_eq!(token_client.balance(&maintainer), 180_0000000i128);
}

// ── Issue #313: get_deposit / reclaim_deposit with a nonexistent deposit index ─

#[test]
fn test_get_deposit_nonexistent_index_returns_deposit_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &1_000_0000000i128);

    // Create a pool with exactly one deposit at index 0.
    client.deposit(&301u64, &sponsor, &token_addr, &100_0000000i128);

    // Index 1 does not exist — must return DepositNotFound.
    let err = client.try_get_deposit(&301u64, &1u32);
    assert_eq!(err, Err(Ok(Error::DepositNotFound)));
}

#[test]
fn test_reclaim_deposit_nonexistent_index_returns_deposit_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &1_000_0000000i128);

    // Create a pool with exactly one deposit at index 0.
    client.deposit(&302u64, &sponsor, &token_addr, &100_0000000i128);

    // Advance past the inactivity window so timing is not the rejection reason.
    env.ledger().set_timestamp(INACTIVITY_WINDOW + 1);

    // Index 1 does not exist — must return DepositNotFound.
    let err = client.try_reclaim_deposit(&302u64, &1u32, &sponsor);
    assert_eq!(err, Err(Ok(Error::DepositNotFound)));
}

// ── Issue #316: reclaim_deposit requires the sponsor's own authorization ──────

#[test]
fn test_reclaim_deposit_requires_sponsor_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &1_000_0000000i128);

    client.deposit(&303u64, &sponsor, &token_addr, &100_0000000i128);

    // Advance past the inactivity window so the timing guard is satisfied.
    env.ledger().set_timestamp(INACTIVITY_WINDOW + 1);

    // Strip all auths — the call must be rejected without the sponsor's signature.
    env.set_auths(&[]);
    let err = client.try_reclaim_deposit(&303u64, &0u32, &sponsor);
    assert!(err.is_err(), "reclaim_deposit must require the sponsor's own authorization");
}

// ── Regression #479: reclaim_deposit second call ─────────────────────────────

#[test]
fn test_reclaim_deposit_second_call_rejected_with_already_reclaimed() {
    // Regression for #479: reclaim_deposit must mark the deposit as reclaimed
    // so a second call on the same index is rejected rather than draining other
    // sponsors' funds.
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &1_000_0000000i128);

    client.deposit(&500u64, &sponsor, &token_addr, &100_0000000i128);

    env.ledger().set_timestamp(INACTIVITY_WINDOW + 1);

    // First reclaim must succeed and transfer the deposit amount.
    client.reclaim_deposit(&500u64, &0u32, &sponsor);
    assert_eq!(token_client.balance(&sponsor), 1_000_0000000i128);

    // Second reclaim on the same index must be rejected.
    let err = client.try_reclaim_deposit(&500u64, &0u32, &sponsor);
    assert_eq!(err, Err(Ok(Error::AlreadyReclaimed)));
}

// ── Regression #472 (closed #308) ─────────────────────────────────────────
// initialize must reject treasury == contract address with InvalidTreasury.
// No mock_all_auths() — admin + oracle auth is meaningful to the test.
#[test]
fn test_initialize_rejects_treasury_equal_to_contract_address() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(&env, &contract_id);

    env.mock_all_auths();
    // Pass the contract's own address as treasury — must be rejected.
    let err = client.try_initialize(&admin, &oracle, &contract_id, &1_000u32, &None);
    assert_eq!(err, Err(Ok(Error::InvalidTreasury)));
}

// ── Regression #329: sweep must reject the contract's own address as recipient ─

#[test]
fn test_sweep_rejects_contract_as_recipient() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    let contract_addr = client.address.clone();

    asset_client.mint(&sponsor, &1_000_0000000i128);
    client.deposit(&400u64, &sponsor, &token_addr, &500_0000000i128);
    // Create surplus so sweep has something to do.
    token_client.transfer(&sponsor, &contract_addr, &100_0000000i128);

    // Sweeping to the contract itself must be rejected.
    let err = client.try_sweep(&400u64, &token_addr, &contract_addr);
    assert_eq!(err, Err(Ok(Error::InvalidTreasury)));
}

// ── Regression #249: set_admin ───────────────────────────────────────────────

#[test]
fn test_set_admin_rotates_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (old_admin, _treasury, client) = setup(&env);

    let new_admin = Address::generate(&env);
    client.set_admin(&new_admin);

    assert_eq!(client.get_admin(), new_admin);
    assert_ne!(client.get_admin(), old_admin);
}

#[test]
fn test_set_admin_rejects_non_admin_caller() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let new_admin = Address::generate(&env);
    env.set_auths(&[]);
    let err = client.try_set_admin(&new_admin);
    assert!(err.is_err());
}

#[test]
fn test_set_admin_requires_new_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let new_admin = Address::generate(&env);
    // set_admin requires both the current admin AND the incoming admin to sign.
    // With full mock_all_auths this succeeds; without it must fail.
    env.set_auths(&[]);
    let err = client.try_set_admin(&new_admin);
    assert!(err.is_err());
}

// ── Regression #44: withdraw must reject zero and negative amounts ────────────

#[test]
fn test_withdraw_rejects_zero_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &500_0000000i128);
    client.deposit(&401u64, &sponsor, &token_addr, &500_0000000i128);

    let maintainer = Address::generate(&env);
    let recipients = vec![&env, (maintainer.clone(), 10_000u32)];
    let err = client.try_withdraw(&401u64, &recipients, &0i128);
    assert_eq!(err, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn test_withdraw_rejects_negative_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &500_0000000i128);
    client.deposit(&402u64, &sponsor, &token_addr, &500_0000000i128);

    let maintainer = Address::generate(&env);
    let recipients = vec![&env, (maintainer.clone(), 10_000u32)];
    let err = client.try_withdraw(&402u64, &recipients, &-1i128);
    assert_eq!(err, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn test_keep_alive_succeeds_on_existing_pool() {
    // Regression for #476: keep_alive must not error on a valid pool.
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &100_0000000i128);
    client.deposit(&600u64, &sponsor, &token_addr, &100_0000000i128);

    // keep_alive is permissionless and must return Ok(()).
    client.keep_alive(&600u64);
}

#[test]
fn test_keep_alive_fails_on_nonexistent_pool() {
    // Regression for #476: keep_alive on an unknown pool_id must return PoolNotFound.
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let err = client.try_keep_alive(&9999u64);
    assert_eq!(err, Err(Ok(Error::PoolNotFound)));
}

// ── Regression #341: initialize must reject treasury == contract address ──────

#[test]
fn test_initialize_rejects_treasury_as_contract_address() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(&env, &contract_id);

    // treasury == contract_id should return InvalidTreasury.
    let err = client.try_initialize(&admin, &oracle, &contract_id, &1_000u32, &None);
    assert_eq!(err, Err(Ok(Error::InvalidTreasury)));
}

#[test]
fn test_initialize_rejects_max_fee_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(&env, &contract_id);

    // fee_bps == 10_001 (> BPS_DENOMINATOR 10_000) should fail.
    let err = client.try_initialize(&admin, &oracle, &treasury, &10_001u32, &None);
    assert_eq!(err, Err(Ok(Error::InvalidFee)));
    let err = client.try_keep_alive(&9999u64);
    assert_eq!(err, Err(Ok(Error::PoolNotFound)));
}

#[contract]
pub struct MockFeeOnTransferToken;

#[contractimpl]
impl MockFeeOnTransferToken {
    pub fn balance(env: Env, id: Address) -> i128 {
        let bal_key = soroban_sdk::Symbol::new(&env, "bal");
        env.storage().persistent().get(&(bal_key, id)).unwrap_or(0)
    }

    pub fn mint(env: Env, to: Address, amount: i128) {
        let bal_key = soroban_sdk::Symbol::new(&env, "bal");
        let balance: i128 = env
            .storage()
            .persistent()
            .get(&(bal_key.clone(), to.clone()))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&(bal_key, to), &(balance + amount));
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        let fee_rate_key = soroban_sdk::Symbol::new(&env, "fee_rate");
        let fee_rate: i128 = env.storage().instance().get(&fee_rate_key).unwrap_or(10); // default 10%
        let fee = (amount * fee_rate) / 100;
        let received = amount - fee;

        let from_key = soroban_sdk::Symbol::new(&env, "bal");
        let to_key = soroban_sdk::Symbol::new(&env, "bal");

        let from_bal: i128 = env
            .storage()
            .persistent()
            .get(&(from_key.clone(), from.clone()))
            .unwrap_or(0);
        let to_bal: i128 = env
            .storage()
            .persistent()
            .get(&(to_key.clone(), to.clone()))
            .unwrap_or(0);

        env.storage()
            .persistent()
            .set(&(from_key, from), &(from_bal - amount));
        if received > 0 {
            env.storage()
                .persistent()
                .set(&(to_key, to), &(to_bal + received));
        }
    }

    pub fn set_fee_rate(env: Env, fee_rate: i128) {
        let fee_rate_key = soroban_sdk::Symbol::new(&env, "fee_rate");
        env.storage().instance().set(&fee_rate_key, &fee_rate);
    }
}

#[test]
fn test_deposit_credits_actual_received_with_fee_on_transfer_token() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_addr = env.register(MockFeeOnTransferToken, ());
    let token_client = MockFeeOnTransferTokenClient::new(&env, &token_addr);

    let sponsor = Address::generate(&env);
    token_client.mint(&sponsor, &1_000i128);

    // Default fee rate is 10%, so depositing 1000 sends 1000 but contract receives 900.
    client.deposit(&100u64, &sponsor, &token_addr, &1_000i128);

    // Bookkeeping must record actual_received (900), not requested amount (1000).
    let pool = client.get_pool(&100u64);
    assert_eq!(pool.balance, 900i128);
    assert_eq!(pool.total_deposited, 900i128);
    assert_eq!(token_client.balance(&client.address), 900i128);

    let deposit = client.get_deposit(&100u64, &0u32);
    assert_eq!(deposit.amount, 900i128);
    assert_eq!(deposit.sponsor, sponsor);
}

#[test]
fn test_deposit_rejects_zero_received_with_fee_on_transfer_token() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_addr = env.register(MockFeeOnTransferToken, ());
    let token_client = MockFeeOnTransferTokenClient::new(&env, &token_addr);

    let sponsor = Address::generate(&env);
    token_client.mint(&sponsor, &1_000i128);

    // Set 100% transfer fee -> 0 received
    token_client.set_fee_rate(&100i128);

    let err = client.try_deposit(&101u64, &sponsor, &token_addr, &500i128);
    assert_eq!(err, Err(Ok(Error::InvalidAmount)));
}
