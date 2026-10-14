#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token, Address, Env,
};

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

#[test]
fn test_get_admin_treasury_fee_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, treasury, client) = setup(&env);

    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_treasury(), treasury);
    assert_eq!(client.get_fee_bps(), 1_000u32);
    assert_eq!(client.get_version(), 1);
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
    client.withdraw(&2u64, &maintainer, &200_0000000i128);

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
    let err = client.try_withdraw(&5u64, &maintainer, &0i128);
    assert_eq!(err, Err(Ok(Error::InvalidAmount)));
    let err = client.try_withdraw(&5u64, &maintainer, &-1i128);
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
    let result = client.try_withdraw(&6u64, &maintainer, &50_0000000i128);
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
            client.withdraw(&8u64, &maintainer, &withdraw_amt);
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
    client.withdraw(&1u64, &maintainer, &100_0000000i128); // 10% fee = 10, payout = 90
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
    let err = client.try_withdraw(&42u64, &maintainer, &1_000_000_000i128);
    assert!(err.is_err());

    // Recovery installs a new admin via the contract entrypoint.
    env.mock_all_auths();
    let new_admin = Address::generate(&env);
    client.recover_admin(&new_admin);
    // New admin withdraws successfully (mocked auth enables it)
    env.mock_all_auths();
    client.withdraw(&42u64, &maintainer, &1_000_000_000i128);
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

    let withdraw_err = client.try_withdraw(&77u64, &maintainer, &1_000_000_000i128);
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
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &1_000_000i128);

    for _ in 0..crate::MAX_DEPOSITS {
        client.deposit(&11u64, &sponsor, &token_addr, &1i128);
    }
    assert_eq!(client.get_pool(&11u64).deposit_count, crate::MAX_DEPOSITS);

    let err = client.try_deposit(&11u64, &sponsor, &token_addr, &1i128);
    assert_eq!(err, Err(Ok(Error::TooManyDeposits)));
    assert_eq!(client.get_pool(&11u64).deposit_count, crate::MAX_DEPOSITS);

    // Other pools are unaffected.
    client.deposit(&12u64, &sponsor, &token_addr, &1i128);
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
fn test_upgrade_bumps_version() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    // Simulate a deployment from before the current CONTRACT_VERSION.
    env.as_contract(&client.address, || {
        env.storage().instance().set(&DataKey::Version, &0u32);
    });
    assert_eq!(client.get_version(), 0);

    client.upgrade(&native_wasm_hash(&env));
    assert_eq!(client.get_version(), CONTRACT_VERSION);
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
    client.withdraw(&246u64, &maintainer, &1_000i128);
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
    client.withdraw(&7u64, &maintainer, &50_0000000i128);

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
    client.withdraw(&253u64, &maintainer, &200_0000000i128);

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

// ── Long-Run Invariant Fuzzing (#538) ──────────────────────────────────────────

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
        let diff = max - min + 1;
        min + (self.next_u32() % diff)
    }
}

fn assert_maintenance_pool_invariants(
    client: &MaintenancePoolContractClient,
    token_client: &token::Client,
    token_addr: &Address,
    tracked_pools: &[u64],
    reclaimed_amounts: &[i128],
) {
    let mut total_pools_balance: i128 = 0;

    for (idx, &pool_id) in tracked_pools.iter().enumerate() {
        if let Ok(Ok(pool)) = client.try_get_pool(&pool_id) {
            if pool.token == *token_addr {
                // Invariant 1: Non-negative balance and lifetime counters
                assert!(pool.balance >= 0, "Pool {} balance {} < 0", pool_id, pool.balance);
                assert!(pool.total_deposited >= 0, "Pool {} total_deposited < 0", pool_id);
                assert!(pool.total_withdrawn >= 0, "Pool {} total_withdrawn < 0", pool_id);

                // Invariant 2: Bookkeeping consistency
                // balance == total_deposited - total_withdrawn - reclaimed_amount
                let reclaimed = reclaimed_amounts[idx];
                let expected_balance = pool
                    .total_deposited
                    .checked_sub(pool.total_withdrawn)
                    .and_then(|b| b.checked_sub(reclaimed))
                    .expect("bookkeeping underflow");

                assert_eq!(
                    pool.balance, expected_balance,
                    "Pool {} balance {} does not match expected total_deposited ({}) - total_withdrawn ({}) - reclaimed ({}) = {}",
                    pool_id, pool.balance, pool.total_deposited, pool.total_withdrawn, reclaimed, expected_balance
                );

                // Invariant 3: Deposit records match total_deposited
                let mut sum_deposits: i128 = 0;
                for d_idx in 0..pool.deposit_count {
                    if let Ok(Ok(deposit)) = client.try_get_deposit(&pool_id, &d_idx) {
                        assert!(deposit.amount > 0, "Deposit amount must be positive");
                        sum_deposits = sum_deposits
                            .checked_add(deposit.amount)
                            .expect("deposit sum overflow");
                    } else {
                        panic!("Missing deposit {} for pool {}", d_idx, pool_id);
                    }
                }
                assert_eq!(
                    sum_deposits, pool.total_deposited,
                    "Pool {} deposit sum {} does not match recorded total_deposited {}",
                    pool_id, sum_deposits, pool.total_deposited
                );

                total_pools_balance = total_pools_balance
                    .checked_add(pool.balance)
                    .expect("total pools balance overflow");
            }
        }
    }

    // Invariant 4: On-chain contract token balance covers all pool balances
    let contract_balance = token_client.balance(&client.address);
    assert!(
        contract_balance >= total_pools_balance,
        "Solvency violation: contract balance {} < total pools balance {}",
        contract_balance,
        total_pools_balance
    );
    assert_eq!(
        contract_balance, total_pools_balance,
        "Exact balance mismatch: contract balance {} != total pools balance {}",
        contract_balance,
        total_pools_balance
    );
}

#[test]
fn test_maintenance_pool_invariant_fuzzing() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);

    let sponsor1 = Address::generate(&env);
    let sponsor2 = Address::generate(&env);
    let sponsor3 = Address::generate(&env);
    let maintainer = Address::generate(&env);

    asset_client.mint(&sponsor1, &100_000_000_000i128);
    asset_client.mint(&sponsor2, &100_000_000_000i128);
    asset_client.mint(&sponsor3, &100_000_000_000i128);

    let sponsors = [&sponsor1, &sponsor2, &sponsor3];

    let tracked_pools = [501u64, 502u64, 503u64, 504u64];
    let mut reclaimed_amounts = [0i128, 0i128, 0i128, 0i128];

    let mut rng = SimpleRng::new(54321);

    // Run 300 randomized actions across deposit, withdraw, reclaim, and time advance
    for _ in 0..300 {
        let action = rng.next_range(0, 4);
        match action {
            0 => {
                // Deposit
                let pool_idx = rng.next_range(0, (tracked_pools.len() - 1) as u32) as usize;
                let pool_id = tracked_pools[pool_idx];
                let sponsor_idx = rng.next_range(0, (sponsors.len() - 1) as u32) as usize;
                let sponsor = sponsors[sponsor_idx];
                let amount = rng.next_range(500, 50_000) as i128;
                let _ = client.try_deposit(&pool_id, sponsor, &token_addr, &amount);
            }
            1 => {
                // Withdraw
                let pool_idx = rng.next_range(0, (tracked_pools.len() - 1) as u32) as usize;
                let pool_id = tracked_pools[pool_idx];
                if let Ok(Ok(pool)) = client.try_get_pool(&pool_id) {
                    if pool.balance > 0 {
                        let amount = rng.next_range(1, pool.balance.min(25_000) as u32) as i128;
                        let _ = client.try_withdraw(&pool_id, &maintainer, &amount);
                    }
                }
            }
            2 => {
                // Reclaim deposit
                let pool_idx = rng.next_range(0, (tracked_pools.len() - 1) as u32) as usize;
                let pool_id = tracked_pools[pool_idx];
                if let Ok(Ok(pool)) = client.try_get_pool(&pool_id) {
                    if pool.deposit_count > 0 {
                        let dep_idx = rng.next_range(0, pool.deposit_count - 1);
                        if let Ok(Ok(dep)) = client.try_get_deposit(&pool_id, &dep_idx) {
                            let res = client.try_reclaim_deposit(&pool_id, &dep_idx, &dep.sponsor);
                            if res.is_ok() {
                                reclaimed_amounts[pool_idx] = reclaimed_amounts[pool_idx]
                                    .checked_add(dep.amount)
                                    .expect("reclaim sum overflow");
                            }
                        }
                    }
                }
            }
            3 => {
                // Advance time (approaching or past INACTIVITY_WINDOW)
                let delta = rng.next_range(1_000, INACTIVITY_WINDOW as u32 / 2) as u64;
                env.ledger().set_timestamp(env.ledger().timestamp() + delta);
            }
            4 => {
                // Keep alive
                let pool_idx = rng.next_range(0, (tracked_pools.len() - 1) as u32) as usize;
                let pool_id = tracked_pools[pool_idx];
                let _ = client.try_keep_alive(&pool_id);
            }
            _ => unreachable!(),
        }

        // Verify all invariants after every single operation
        assert_maintenance_pool_invariants(
            &client,
            &token_client,
            &token_addr,
            &tracked_pools,
            &reclaimed_amounts,
        );
    }
}
