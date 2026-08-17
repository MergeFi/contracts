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
    let contract_id = env.register(
        MilestonesContract,
        MilestonesContractArgs::__constructor(&admin, &treasury, &500u32),
    );
    let client = MilestonesContractClient::new(env, &contract_id);
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
#[should_panic]
fn test_constructor_requires_admin_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    env.register(
        MilestonesContract,
        MilestonesContractArgs::__constructor(&admin, &treasury, &500u32),
    );
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

// ---------------------------------------------------------------------------
// Multi-sponsor crowdfunding (#58)
// ---------------------------------------------------------------------------

#[test]
fn test_multi_sponsor_milestone_proportional_refund_after_partial_allocation() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);

    let sponsor_a = Address::generate(&env);
    let sponsor_b = Address::generate(&env);
    // Each sponsor is minted exactly their contribution, so post-refund
    // balances directly show what came back with no other funds involved.
    asset_client.mint(&sponsor_a, &700i128);
    asset_client.mint(&sponsor_b, &300i128);

    // Sponsor A opens the milestone with 700; sponsor B co-funds with 300.
    client.create_milestone(&50u64, &sponsor_a, &token_addr, &700i128);
    client.contribute(&50u64, &sponsor_b, &300i128);

    let milestone = client.get_milestone(&50u64);
    assert_eq!(milestone.total_budget, 1_000i128);
    assert_eq!(milestone.remaining_budget, 1_000i128);
    assert_eq!(milestone.contributor_count, 2);

    // 400 of the 1000 is allocated to a real issue and released (the fee
    // and recipient payout draw on the allocation, so `remaining_budget`
    // stays at 600).
    client.allocate(&50u64, &501u64, &400i128);
    let maintainer = Address::generate(&env);
    client.release_issue(&50u64, &501u64, &vec![&env, (maintainer, 10_000u32)]);

    client.cancel_milestone(&50u64);

    // Remaining budget is 600; it is refunded in proportion to what each
    // sponsor contributed (70/30 of the *unspent* remainder) — not an even
    // split of the 600, and not 70/30 of the original 1000 nominal total.
    assert_eq!(token_client.balance(&sponsor_a), 420i128); // 70% of 600
    assert_eq!(token_client.balance(&sponsor_b), 180i128); // 30% of 600
    assert_eq!(client.get_milestone(&50u64).remaining_budget, 0);
}

#[test]
fn test_multi_sponsor_refund_rounds_dust_by_largest_remainder() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    asset_client.mint(&a, &3i128);
    asset_client.mint(&b, &3i128);
    asset_client.mint(&c, &4i128);

    client.create_milestone(&51u64, &a, &token_addr, &3i128);
    client.contribute(&51u64, &b, &3i128);
    client.contribute(&51u64, &c, &4i128);

    // Leave 7 of the 10 unallocated: 7*3/10 = 2 (rem 1), 7*3/10 = 2 (rem 1),
    // 7*4/10 = 2 (rem 8) -> the single dust unit goes to the largest
    // remainder, i.e. c, so the refund is 2 + 2 + 3 = 7 and nothing is
    // stranded in the contract.
    client.allocate(&51u64, &511u64, &3i128);
    client.cancel_milestone(&51u64);

    assert_eq!(token_client.balance(&a), 2i128);
    assert_eq!(token_client.balance(&b), 2i128);
    assert_eq!(token_client.balance(&c), 3i128);
}

#[test]
fn test_contribute_grows_pool_and_remaining_budget() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);

    client.create_milestone(&52u64, &alice, &token_addr, &1_000i128);
    // The same sponsor (or anyone) can top up: new funds arrive unallocated,
    // so both totals grow together.
    client.contribute(&52u64, &alice, &500i128);

    let milestone = client.get_milestone(&52u64);
    assert_eq!(milestone.total_budget, 1_500i128);
    assert_eq!(milestone.remaining_budget, 1_500i128);
    assert_eq!(milestone.contributor_count, 2);
}

#[test]
fn test_contribute_requires_sponsor_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);
    asset_client.mint(&bob, &10_000i128);
    client.create_milestone(&53u64, &alice, &token_addr, &5_000i128);

    // No auth provided for bob's contribution.
    env.set_auths(&[]);
    let result = client.try_contribute(&53u64, &bob, &5_000i128);
    assert!(result.is_err());
}

#[test]
fn test_contribute_rejects_invalid_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);
    asset_client.mint(&bob, &10_000i128);
    client.create_milestone(&54u64, &alice, &token_addr, &5_000i128);

    let err = client.try_contribute(&54u64, &bob, &0i128);
    assert_eq!(err, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn test_contribute_rejects_unknown_milestone() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let bob = Address::generate(&env);
    let err = client.try_contribute(&999u64, &bob, &1_000i128);
    assert_eq!(err, Err(Ok(Error::MilestoneNotFound)));
}

#[test]
fn test_contribute_rejects_after_closed() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);
    asset_client.mint(&bob, &10_000i128);
    client.create_milestone(&55u64, &alice, &token_addr, &5_000i128);
    client.cancel_milestone(&55u64);

    let err = client.try_contribute(&55u64, &bob, &1_000i128);
    assert_eq!(err, Err(Ok(Error::MilestoneClosed)));
}

#[test]
fn test_contribute_rejects_beyond_max_sponsors() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    asset_client.mint(&alice, &100_000i128);
    client.create_milestone(&56u64, &alice, &token_addr, &1_000i128);

    // MAX_SPONSORS is 20; alice's `create_milestone` above already used
    // slot 0, so 19 more `contribute` calls exactly fill the cap.
    for _ in 0..(crate::MAX_SPONSORS - 1) {
        let extra = Address::generate(&env);
        asset_client.mint(&extra, &1_000i128);
        client.contribute(&56u64, &extra, &1_000i128);
    }
    assert_eq!(
        client.get_milestone(&56u64).contributor_count,
        crate::MAX_SPONSORS
    );

    // The 21st distinct contribution is rejected.
    let one_too_many = Address::generate(&env);
    asset_client.mint(&one_too_many, &1_000i128);
    let err = client.try_contribute(&56u64, &one_too_many, &1_000i128);
    assert_eq!(err, Err(Ok(Error::TooManySponsors)));
}

#[test]
fn test_get_contribution_enumerates_each_contributor() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);
    asset_client.mint(&bob, &10_000i128);

    client.create_milestone(&57u64, &alice, &token_addr, &4_000i128);
    client.contribute(&57u64, &bob, &6_000i128);

    let c0 = client.get_contribution(&57u64, &0u32);
    let c1 = client.get_contribution(&57u64, &1u32);
    assert_eq!(c0.sponsor, alice);
    assert_eq!(c0.amount, 4_000i128);
    assert_eq!(c1.sponsor, bob);
    assert_eq!(c1.amount, 6_000i128);

    let err = client.try_get_contribution(&57u64, &2u32);
    assert_eq!(err, Err(Ok(Error::MilestoneNotFound)));
}
