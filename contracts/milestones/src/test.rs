#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, token, vec, Address, Env};

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

fn setup(env: &Env) -> (Address, Address, MilestonesContractClient<'_>) {
    let admin = Address::generate(env);
    let treasury = Address::generate(env);
    let contract_id = env.register(MilestonesContract, ());
    let client = MilestonesContractClient::new(env, &contract_id);
    client.initialize(&admin, &treasury, &500u32); // 5% fee
    (admin, treasury, client)
}

#[test]
fn test_create_milestone_allocate_and_release_per_issue() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &1_000_0000000i128);

    client.create_milestone(&1u64, &sponsor, &token_addr, &1_000_0000000i128);

    // Allocate budget across two issues.
    client.allocate(&1u64, &101u64, &600_0000000i128);
    client.allocate(&1u64, &102u64, &400_0000000i128);

    let milestone = client.get_milestone(&1u64);
    assert_eq!(milestone.remaining_budget, 0);

    let contributor_a = Address::generate(&env);
    let contributor_b = Address::generate(&env);

    client.release_issue(&1u64, &101u64, &vec![&env, (contributor_a.clone(), 10_000u32)]);
    client.release_issue(&1u64, &102u64, &vec![&env, (contributor_b.clone(), 10_000u32)]);

    // 5% fee on each release.
    assert_eq!(token_client.balance(&contributor_a), 570_0000000i128);
    assert_eq!(token_client.balance(&contributor_b), 380_0000000i128);
    assert_eq!(token_client.balance(&treasury), 30_0000000i128 + 20_0000000i128);
}

#[test]
fn test_allocate_rejects_over_allocation() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &1_000_0000000i128);

    client.create_milestone(&2u64, &sponsor, &token_addr, &1_000_0000000i128);
    client.allocate(&2u64, &201u64, &700_0000000i128);

    let err = client.try_allocate(&2u64, &202u64, &400_0000000i128);
    assert_eq!(err, Err(Ok(Error::OverAllocation)));
}

#[test]
fn test_release_issue_rejects_double_release() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &1_000_0000000i128);

    client.create_milestone(&3u64, &sponsor, &token_addr, &1_000_0000000i128);
    client.allocate(&3u64, &301u64, &500_0000000i128);

    let contributor = Address::generate(&env);
    client.release_issue(&3u64, &301u64, &vec![&env, (contributor.clone(), 10_000u32)]);

    let err = client.try_release_issue(&3u64, &301u64, &vec![&env, (contributor, 10_000u32)]);
    assert_eq!(err, Err(Ok(Error::IssueAlreadyReleased)));
}

#[test]
fn test_cancel_milestone_refunds_remaining_budget() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &1_000_0000000i128);

    client.create_milestone(&4u64, &sponsor, &token_addr, &1_000_0000000i128);
    client.allocate(&4u64, &401u64, &300_0000000i128);

    client.cancel_milestone(&4u64);

    // 700 remaining budget refunded to sponsor (sponsor started with 0 after
    // deposit, so balance should now equal the unallocated remainder).
    assert_eq!(token_client.balance(&sponsor), 700_0000000i128);
    assert!(client.get_milestone(&4u64).closed);
}
