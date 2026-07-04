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
    client.initialize(&admin, &treasury, &1_000u32); // 10% fee
    (admin, treasury, client)
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
