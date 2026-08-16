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
    asset_client.mint(&sponsor, &10_000_000_000i128);

    client.create_milestone(&1u64, &sponsor, &token_addr, &10_000_000_000i128);

    // Allocate budget across two issues.
    client.allocate(&1u64, &101u64, &600_0000000i128);
    client.allocate(&1u64, &102u64, &400_0000000i128);

    let milestone = client.get_milestone(&1u64);
    assert_eq!(milestone.remaining_budget, 0);

    let contributor_a = Address::generate(&env);
    let contributor_b = Address::generate(&env);

    client.release_issue(
        &1u64,
        &101u64,
        &vec![&env, (contributor_a.clone(), 10_000u32)],
    );
    client.release_issue(
        &1u64,
        &102u64,
        &vec![&env, (contributor_b.clone(), 10_000u32)],
    );

    // 5% fee on each release.
    assert_eq!(token_client.balance(&contributor_a), 570_0000000i128);
    assert_eq!(token_client.balance(&contributor_b), 380_0000000i128);
    assert_eq!(
        token_client.balance(&treasury),
        30_0000000i128 + 20_0000000i128
    );
}

#[test]
fn test_release_issue_distributes_rounding_dust_by_largest_remainder() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &101i128);

    client.create_milestone(&5u64, &sponsor, &token_addr, &101i128);
    client.allocate(&5u64, &501u64, &101i128);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let carol = Address::generate(&env);
    let recipients = vec![
        &env,
        (alice.clone(), 3_334u32),
        (bob.clone(), 3_333u32),
        (carol.clone(), 3_333u32),
    ];
    client.release_issue(&5u64, &501u64, &recipients);

    assert_eq!(token_client.balance(&treasury), 5i128);
    assert_eq!(token_client.balance(&alice), 32i128);
    assert_eq!(token_client.balance(&bob), 32i128);
    assert_eq!(token_client.balance(&carol), 32i128);
}

#[test]
fn test_large_split_distributes_dust_by_largest_remainder() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(crate::MilestonesContract, ());
    let client = crate::MilestonesContractClient::new(&env, &contract_id);
    // 0% fee so the whole total is distributable.
    client.initialize(&admin, &treasury, &0u32);

    // 60 recipients: 59 with alternating 160/170 bps, the last one receiving
    // the leftover of 10000. All 170-bps recipients share an identical
    // remainder, so most of the dust has to be resolved by the address-based
    // tie-break, exercising both the O(n log n) sort and the tie-break at
    // scale.
    let mut recipients = Vec::new(&env);
    let mut total_bps: u32 = 0;
    for i in 0..59u32 {
        let bps = if i % 5 == 0 { 160 } else { 170 };
        recipients.push_back((Address::generate(&env), bps));
        total_bps += bps;
    }
    let last_bps = BPS_DENOMINATOR as u32 - total_bps;
    recipients.push_back((Address::generate(&env), last_bps));

    // Chosen so that integer division leaves exactly 40 dust units to
    // distribute.
    let total: i128 = 123_457;
    let payouts = env.as_contract(&contract_id, || {
        compute_split(&env, total, &recipients).unwrap()
    });

    // Reference result computed with the previous O(n²) repeated
    // largest-remainder scan; the new implementation must match it exactly.
    let mut expected: Vec<i128> = Vec::new(&env);
    let mut remainders: Vec<i128> = Vec::new(&env);
    let mut allocated: i128 = 0;
    for (_, bps) in recipients.iter() {
        let numerator = total * (bps as i128);
        let share = numerator / BPS_DENOMINATOR;
        let remainder = numerator % BPS_DENOMINATOR;
        allocated += share;
        expected.push_back(share);
        remainders.push_back(remainder);
    }
    let mut dust = total - allocated;
    assert!(
        dust >= 2,
        "test must exercise multiple dust units, got {dust}"
    );
    while dust > 0 {
        let mut best_index: u32 = 0;
        let mut best_remainder: i128 = -1;
        for (i, remainder) in remainders.iter().enumerate() {
            if remainder > best_remainder {
                best_index = i as u32;
                best_remainder = remainder;
            } else if remainder == best_remainder && remainder != -1 {
                let current_addr = recipients.get(i as u32).unwrap().0;
                let best_addr = recipients.get(best_index).unwrap().0;
                if current_addr < best_addr {
                    best_index = i as u32;
                    best_remainder = remainder;
                }
            }
        }
        expected.set(best_index, expected.get(best_index).unwrap() + 1);
        remainders.set(best_index, -1);
        dust -= 1;
    }

    let mut total_paid: i128 = 0;
    for (i, _) in recipients.iter().enumerate() {
        let (_, share) = payouts.shares.get(i as u32).unwrap();
        assert_eq!(share, expected.get(i as u32).unwrap(), "recipient {i}");
        total_paid += share;
    }
    assert_eq!(
        total_paid, total,
        "all distributable funds must be paid out"
    );
}

#[test]
fn test_allocate_rejects_over_allocation() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    client.create_milestone(&2u64, &sponsor, &token_addr, &10_000_000_000i128);
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
    asset_client.mint(&sponsor, &10_000_000_000i128);

    client.create_milestone(&3u64, &sponsor, &token_addr, &10_000_000_000i128);
    client.allocate(&3u64, &301u64, &500_0000000i128);

    let contributor = Address::generate(&env);
    client.release_issue(
        &3u64,
        &301u64,
        &vec![&env, (contributor.clone(), 10_000u32)],
    );

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
    asset_client.mint(&sponsor, &10_000_000_000i128);

    client.create_milestone(&4u64, &sponsor, &token_addr, &10_000_000_000i128);
    client.allocate(&4u64, &401u64, &300_0000000i128);

    client.cancel_milestone(&4u64);

    // 700 remaining budget refunded to sponsor (sponsor started with 0 after
    // deposit, so balance should now equal the unallocated remainder).
    assert_eq!(token_client.balance(&sponsor), 700_0000000i128);
    assert!(client.get_milestone(&4u64).closed);
}

// ---------------------------------------------------------------------------
// Access-control boundary matrix (#30)
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_requires_admin_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(MilestonesContract, ());
    let client = MilestonesContractClient::new(&env, &contract_id);

    let result = client.try_initialize(&admin, &treasury, &500u32);
    assert!(result.is_err());
}

#[test]
fn test_create_milestone_requires_sponsor_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    env.set_auths(&[]);
    let result = client.try_create_milestone(&6u64, &sponsor, &token_addr, &10_000_000_000i128);
    assert!(result.is_err());
}

#[test]
fn test_allocate_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);
    client.create_milestone(&7u64, &sponsor, &token_addr, &10_000_000_000i128);

    env.set_auths(&[]);
    let result = client.try_allocate(&7u64, &701u64, &100_0000000i128);
    assert!(result.is_err());
}

#[test]
fn test_release_issue_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);
    client.create_milestone(&8u64, &sponsor, &token_addr, &10_000_000_000i128);
    client.allocate(&8u64, &801u64, &100_0000000i128);

    env.set_auths(&[]);
    let contributor = Address::generate(&env);
    let result = client.try_release_issue(&8u64, &801u64, &vec![&env, (contributor, 10_000u32)]);
    assert!(result.is_err());
}

#[test]
fn test_cancel_milestone_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);
    client.create_milestone(&9u64, &sponsor, &token_addr, &10_000_000_000i128);

    env.set_auths(&[]);
    let result = client.try_cancel_milestone(&9u64);
    assert!(result.is_err());
}
