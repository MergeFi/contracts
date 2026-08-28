#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, token, Address, Env};

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
    let treasury = Address::generate(env);
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(env, &contract_id);
    client.initialize(&admin, &treasury, &1_000u32, &None); // 10% fee
    (admin, treasury, client)
}

#[test]
fn test_get_admin_treasury_fee_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, treasury, client) = setup(&env);

    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_treasury(), treasury);
    assert_eq!(client.get_fee_bps(), 1_000u32);
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

    let withdraw_err = client.try_withdraw(&404u64, &maintainer, &1i128);
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
    client.withdraw(&2u64, &maintainer, &200_0000000i128);

    // 10% fee -> 20_0000000 to treasury, 180_0000000 to maintainer.
    assert_eq!(token_client.balance(&maintainer), 180_0000000i128);
    assert_eq!(token_client.balance(&treasury), 20_0000000i128);

    let pool = client.get_pool(&2u64);
    assert_eq!(pool.balance, 800_0000000i128);
    assert_eq!(pool.total_withdrawn, 200_0000000i128);
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
    let err = client.try_withdraw(&3u64, &maintainer, &200_0000000i128);
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
    let treasury = Address::generate(&env);
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(&env, &contract_id);

    let result = client.try_initialize(&admin, &treasury, &1_000u32, &None);
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
    let result = client.try_withdraw(&6u64, &maintainer, &50_0000000i128);
    assert!(result.is_err());
}

#[test]
fn test_initialize_rejects_double_init() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(&env, &contract_id);

    // First initialization succeeds
    client.initialize(&admin, &treasury, &1_000u32, &None);

    // Second initialization should fail with AlreadyInitialized
    let result = client.try_initialize(&admin, &treasury, &1_000u32, &None);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_initialize_rejects_invalid_fee() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(&env, &contract_id);

    // fee_bps > 10000 should fail with InvalidFee
    let result = client.try_initialize(&admin, &treasury, &10_001u32, &None);
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

    // One more than the exact balance must be rejected.
    let err = client.try_withdraw(&7u64, &maintainer, &500_0000001i128);
    assert_eq!(err, Err(Ok(Error::InsufficientBalance)));

    // Exactly the pool's balance must succeed and drain it to zero.
    client.withdraw(&7u64, &maintainer, &500_0000000i128);
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
    asset_client.mint(&sponsor, &1_800_0000000i128);

    let mut total_deposited = 0i128;
    let mut total_withdrawn = 0i128;

    for (deposit_amt, withdraw_amt) in [
        (1_000_0000000i128, 200_0000000i128),
        (500_0000000i128, 100_0000000i128),
        (300_0000000i128, 0i128),
    ] {
        client.deposit(&8u64, &sponsor, &token_addr, &deposit_amt);
        total_deposited += deposit_amt;

        let pool = client.get_pool(&8u64);
        assert_eq!(pool.total_deposited, total_deposited);
        assert_eq!(pool.balance, total_deposited - total_withdrawn);

        if withdraw_amt > 0 {
            client.withdraw(&8u64, &maintainer, &withdraw_amt);
            total_withdrawn += withdraw_amt;

            let pool = client.get_pool(&8u64);
            assert_eq!(pool.total_withdrawn, total_withdrawn);
            assert_eq!(pool.balance, total_deposited - total_withdrawn);
        }
    }

    let pool = client.get_pool(&8u64);
    assert_eq!(pool.total_deposited, 1_800_0000000i128);
    assert_eq!(pool.total_withdrawn, 300_0000000i128);
    assert_eq!(pool.balance, 1_500_0000000i128);
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
fn test_recover_withdraw_frozen_before_recoverable_after() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let recovery = Address::generate(&env);
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(&env, &contract_id);

    // Initialize with a recovery address
    env.mock_all_auths();
    client.initialize(&admin, &treasury, &1_000u32, &Some(recovery.clone()));

    // Deposit into pool
    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &1_000_0000000i128);
    client.deposit(&42u64, &sponsor, &token_addr, &1_000_0000000i128);

    // Simulate lost admin: clear auths
    env.set_auths(&[]);
    let maintainer = Address::generate(&env);
    let err = client.try_withdraw(&42u64, &maintainer, &1_000_000000i128);
    assert!(err.is_err());

    // Recovery installs a new admin via the contract entrypoint.
    env.mock_all_auths();
    let new_admin = Address::generate(&env);
    client.recover_admin(&new_admin);
    // New admin withdraws successfully (mocked auth enables it)
    env.mock_all_auths();
    client.withdraw(&42u64, &maintainer, &1_000_000000i128);
    assert_eq!(token_client.balance(&maintainer), 900_000000i128); // after 10% fee
}
