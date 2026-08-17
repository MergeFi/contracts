#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, vec, Address, Env,
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

fn setup(env: &Env) -> (Address, Address, Address, EscrowContractClient<'_>) {
    let admin = Address::generate(env);
    let treasury = Address::generate(env);
    let contract_id = env.register(
        EscrowContract,
        EscrowContractArgs::__constructor(&admin, &treasury, &500u32),
    );
    let client = EscrowContractClient::new(env, &contract_id);
    (contract_id, admin, treasury, client)
}

#[test]
fn test_constructor_sets_configuration() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, admin, treasury, client) = setup(&env);
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_treasury(), treasury);
    assert_eq!(client.get_fee_bps(), 500u32);
}

#[test]
fn test_fund_and_release_single_recipient() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    let contributor = Address::generate(&env);

    client.fund(&1u64, &sponsor, &token_addr, &10_000_000_000i128, &1_000u64);

    let escrow = client.get_escrow(&1u64);
    assert_eq!(escrow.amount, 10_000_000_000i128);
    assert_eq!(escrow.status, EscrowStatus::Funded);
    assert_eq!(token_client.balance(&contributor), 0);

    let recipients = vec![&env, (contributor.clone(), 10_000u32)];
    client.release(&1u64, &recipients);

    let escrow = client.get_escrow(&1u64);
    assert_eq!(escrow.status, EscrowStatus::Paid);

    // 5% fee -> 50_0000000, contributor gets 950_0000000
    assert_eq!(token_client.balance(&contributor), 950_0000000i128);
    assert_eq!(token_client.balance(&treasury), 50_0000000i128);
}

#[test]
fn test_release_with_team_split() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.fund(&2u64, &sponsor, &token_addr, &10_000_000_000i128, &1_000u64);

    // 60/40 split, 5% fee off the top
    let recipients = vec![&env, (alice.clone(), 6_000u32), (bob.clone(), 4_000u32)];
    client.release(&2u64, &recipients);

    let distributable = 950_0000000i128; // after 5% fee
    let alice_expected = distributable * 6000 / 10000;
    let bob_expected = distributable - alice_expected; // remainder goes to last recipient
    assert_eq!(token_client.balance(&alice), alice_expected);
    assert_eq!(token_client.balance(&bob), bob_expected);
    assert_eq!(token_client.balance(&treasury), 50_0000000i128);
}

#[test]
fn test_release_distributes_rounding_dust_by_largest_remainder() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &101i128);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let carol = Address::generate(&env);

    client.fund(&8u64, &sponsor, &token_addr, &101i128, &1_000u64);

    let recipients = vec![
        &env,
        (alice.clone(), 3_334u32),
        (bob.clone(), 3_333u32),
        (carol.clone(), 3_333u32),
    ];
    client.release(&8u64, &recipients);

    assert_eq!(token_client.balance(&treasury), 5i128);
    assert_eq!(token_client.balance(&alice), 32i128);
    assert_eq!(token_client.balance(&bob), 32i128);
    assert_eq!(token_client.balance(&carol), 32i128);
}

#[test]
fn test_release_rejects_invalid_split() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.fund(&3u64, &sponsor, &token_addr, &10_000_000_000i128, &1_000u64);

    // Splits sum to 9000, not 10000 -> invalid
    let recipients = vec![&env, (alice.clone(), 5_000u32), (bob.clone(), 4_000u32)];
    let err = client.try_release(&3u64, &recipients);
    assert_eq!(err, Err(Ok(Error::InvalidSplit)));
}

#[test]
fn test_double_release_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);
    let contributor = Address::generate(&env);

    client.fund(&4u64, &sponsor, &token_addr, &10_000_000_000i128, &1_000u64);
    let recipients = vec![&env, (contributor.clone(), 10_000u32)];
    client.release(&4u64, &recipients);

    let err = client.try_release(&4u64, &recipients);
    assert_eq!(err, Err(Ok(Error::AlreadyPaid)));
}

#[test]
fn test_unauthorized_release_rejected() {
    let env = Env::default();
    // Construction/fund both need auth, so mock broadly up front and
    // turn it off only for the specific unauthorized call under test below.
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);

    asset_client.mint(&sponsor, &10_000_000_000i128);
    client.fund(&5u64, &sponsor, &token_addr, &10_000_000_000i128, &1_000u64);

    // Turn auth mocking off; release requires admin auth which is not
    // provided here, so it must fail with an auth error.
    env.set_auths(&[]);
    let contributor = Address::generate(&env);
    let recipients = vec![&env, (contributor.clone(), 10_000u32)];
    let result = client.try_release(&5u64, &recipients);
    assert!(result.is_err());
}

#[test]
fn test_refund_after_deadline() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    env.ledger().set_timestamp(100);

    client.fund(&6u64, &sponsor, &token_addr, &10_000_000_000i128, &200u64);

    // Before deadline: admin can still force refund (mock_all_auths covers it).
    env.ledger().set_timestamp(150);
    client.refund(&6u64);
    assert_eq!(token_client.balance(&sponsor), 10_000_000_000i128);
    assert_eq!(client.get_escrow(&6u64).status, EscrowStatus::Refunded);
}

#[test]
fn test_refund_rejected_if_already_paid() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);
    let contributor = Address::generate(&env);

    client.fund(&7u64, &sponsor, &token_addr, &10_000_000_000i128, &1_000u64);
    let recipients = vec![&env, (contributor.clone(), 10_000u32)];
    client.release(&7u64, &recipients);

    let err = client.try_refund(&7u64);
    assert_eq!(err, Err(Ok(Error::AlreadyPaid)));
}

#[test]
fn test_adversarial_ordering_resistance() {
    let env = Env::default();
    env.mock_all_auths();

    // 1. Setup contract and environment
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(
        crate::EscrowContract,
        crate::EscrowContractArgs::__constructor(&admin, &treasury, &0u32),
    );
    // 2. Create recipient addresses
    let dev1 = Address::generate(&env);
    let dev2 = Address::generate(&env);
    let attacker = Address::generate(&env);

    let total_amount: i128 = 10_000_000;

    // 3. Normal ordering (attacker at the beginning)
    let mut normal_order = Vec::new(&env);
    normal_order.push_back((attacker.clone(), 3333u32));
    normal_order.push_back((dev1.clone(), 3333u32));
    normal_order.push_back((dev2.clone(), 3334u32));

    // compute_split reads FeeBps from instance storage, which is only
    // accessible while "inside" the contract that owns it (env.as_contract
    // wraps the closure with that context — calling it directly from the
    // test, as before, panics with "not accessible outside of a contract").
    let payouts_normal = env.as_contract(&contract_id, || {
        crate::compute_split(&env, total_amount, &normal_order).unwrap()
    });

    // 4. Malicious ordering (attacker at the end to steal the remainder)
    let mut malicious_order = Vec::new(&env);
    malicious_order.push_back((dev1.clone(), 3333u32));
    malicious_order.push_back((dev2.clone(), 3334u32));
    malicious_order.push_back((attacker.clone(), 3333u32));

    let payouts_malicious = env.as_contract(&contract_id, || {
        crate::compute_split(&env, total_amount, &malicious_order).unwrap()
    });

    // 5. Extract the attacker's share in both scenarios
    let mut attacker_share_normal = 0;
    for (addr, share) in payouts_normal.shares.iter() {
        if addr == attacker {
            attacker_share_normal = share;
        }
    }

    let mut attacker_share_malicious = 0;
    for (addr, share) in payouts_malicious.shares.iter() {
        if addr == attacker {
            attacker_share_malicious = share;
        }
    }

    // 6. Assert that the result is identical regardless of the order
    assert_eq!(
        attacker_share_normal, attacker_share_malicious,
        "Adversarial ordering exploit failed! Payouts must be order-independent."
    );
}

// ---------------------------------------------------------------------------
// Access-control boundary matrix (#30)
// ---------------------------------------------------------------------------

#[test]
#[should_panic]
fn test_constructor_requires_admin_auth() {
    let env = Env::default();
    // No auths mocked at all.
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    env.register(
        EscrowContract,
        EscrowContractArgs::__constructor(&admin, &treasury, &500u32),
    );
}

#[test]
fn test_fund_requires_sponsor_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    // No sponsor auth provided for this specific call.
    env.set_auths(&[]);
    let result = client.try_fund(&9u64, &sponsor, &token_addr, &10_000_000_000i128, &1_000u64);
    assert!(result.is_err());
}

#[test]
fn test_refund_before_deadline_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    env.ledger().set_timestamp(100);
    client.fund(&10u64, &sponsor, &token_addr, &10_000_000_000i128, &200u64);

    // Still before deadline (100 < 200), and no auth provided at all.
    env.set_auths(&[]);
    let result = client.try_refund(&10u64);
    assert!(result.is_err());
}

#[test]
fn test_refund_after_deadline_is_permissionless() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    env.ledger().set_timestamp(100);
    client.fund(&11u64, &sponsor, &token_addr, &10_000_000_000i128, &200u64);

    // Past the deadline + grace period, and with every auth turned off — not even the
    // sponsor or admin authorizes this call. `refund` must still succeed:
    // this is the "anyone" path the whole design exists to provide.
    env.ledger().set_timestamp(200 + crate::GRACE_PERIOD);
    env.set_auths(&[]);
    client.refund(&11u64);

    assert_eq!(token_client.balance(&sponsor), 10_000_000_000i128);
    assert_eq!(client.get_escrow(&11u64).status, EscrowStatus::Refunded);
}

#[test]
fn test_extend_deadline_requires_sponsor_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    env.ledger().set_timestamp(100);
    client.fund(&12u64, &sponsor, &token_addr, &10_000_000_000i128, &200u64);

    // Not even the admin can extend on the sponsor's behalf.
    env.set_auths(&[]);
    let result = client.try_extend_deadline(&12u64, &sponsor, &500u64);
    assert!(result.is_err());
}

#[test]
fn test_extend_deadline_pushes_out_the_permissionless_window() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    env.ledger().set_timestamp(100);
    client.fund(&13u64, &sponsor, &token_addr, &10_000_000_000i128, &200u64);

    client.extend_deadline(&13u64, &sponsor, &500u64);
    assert_eq!(client.get_escrow(&13u64).deadline, 500u64);

    // Old deadline (200) has now passed, but the extended one (500) hasn't:
    // refund must still require admin auth, proving the extension actually
    // re-closed the permissionless window.
    env.ledger().set_timestamp(200 + crate::GRACE_PERIOD);
    env.set_auths(&[]);
    let result = client.try_refund(&13u64);
    assert!(result.is_err());
}

#[test]
fn test_extend_deadline_rejects_non_increasing_deadline() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    env.ledger().set_timestamp(100);
    client.fund(&14u64, &sponsor, &token_addr, &10_000_000_000i128, &200u64);

    // Equal to the current deadline: rejected.
    let err = client.try_extend_deadline(&14u64, &sponsor, &200u64);
    assert_eq!(err, Err(Ok(Error::InvalidDeadline)));

    // Earlier than the current deadline: rejected.
    let err = client.try_extend_deadline(&14u64, &sponsor, &150u64);
    assert_eq!(err, Err(Ok(Error::InvalidDeadline)));

    // Later than the current deadline but not later than "now": rejected.
    env.ledger().set_timestamp(250);
    let err = client.try_extend_deadline(&14u64, &sponsor, &201u64);
    assert_eq!(err, Err(Ok(Error::InvalidDeadline)));
}

#[test]
fn test_extend_deadline_rejects_after_paid_or_refunded() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);
    let contributor = Address::generate(&env);

    client.fund(
        &15u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
    );
    client.release(&15u64, &vec![&env, (contributor, 10_000u32)]);

    let err = client.try_extend_deadline(&15u64, &sponsor, &2_000u64);
    assert_eq!(err, Err(Ok(Error::AlreadyPaid)));
}

// ---------------------------------------------------------------------------
// Crowdfunding (#57)
// ---------------------------------------------------------------------------

#[test]
fn test_multi_sponsor_refund_returns_exact_contributions_to_each_sponsor() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let carol = Address::generate(&env);
    // Each sponsor is minted exactly their contribution amount, so their
    // post-refund balance is a direct check of "did the correct amount
    // come back" with no other funds to obscure it.
    asset_client.mint(&alice, &3_000i128);
    asset_client.mint(&bob, &7_000i128);
    asset_client.mint(&carol, &1_500i128);

    env.ledger().set_timestamp(100);

    // Three different sponsors co-fund the same issue with three different
    // (deliberately unequal) amounts.
    client.fund(&100u64, &alice, &token_addr, &3_000i128, &200u64);
    client.contribute(&100u64, &bob, &7_000i128);
    client.contribute(&100u64, &carol, &1_500i128);

    let escrow = client.get_escrow(&100u64);
    assert_eq!(escrow.amount, 11_500i128);
    assert_eq!(escrow.contributor_count, 3);

    // Past the deadline and grace period: permissionless refund.
    env.ledger().set_timestamp(200 + crate::GRACE_PERIOD);
    env.set_auths(&[]);
    client.refund(&100u64);

    // Each sponsor gets back exactly what they put in — not an even split
    // (11_500 / 3) and not the full amount to only one of them.
    assert_eq!(token_client.balance(&alice), 3_000i128);
    assert_eq!(token_client.balance(&bob), 7_000i128);
    assert_eq!(token_client.balance(&carol), 1_500i128);
    assert_eq!(client.get_escrow(&100u64).status, EscrowStatus::Refunded);
}

#[test]
fn test_multi_sponsor_release_pays_out_the_combined_total() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);
    asset_client.mint(&bob, &10_000i128);

    client.fund(&101u64, &alice, &token_addr, &4_000i128, &1_000u64);
    client.contribute(&101u64, &bob, &6_000i128);

    let maintainer = Address::generate(&env);
    client.release(&101u64, &vec![&env, (maintainer.clone(), 10_000u32)]);

    // 5% fee off the combined 10_000 total, same as a single-sponsor release.
    assert_eq!(token_client.balance(&treasury), 500i128);
    assert_eq!(token_client.balance(&maintainer), 9_500i128);
    assert_eq!(client.get_escrow(&101u64).status, EscrowStatus::Paid);
}

#[test]
fn test_contribute_requires_sponsor_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);
    asset_client.mint(&bob, &10_000i128);

    client.fund(&102u64, &alice, &token_addr, &5_000i128, &1_000u64);

    // No auth provided for bob's contribution.
    env.set_auths(&[]);
    let result = client.try_contribute(&102u64, &bob, &5_000i128);
    assert!(result.is_err());
}

#[test]
fn test_contribute_rejects_invalid_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);

    client.fund(&103u64, &alice, &token_addr, &5_000i128, &1_000u64);

    let err = client.try_contribute(&103u64, &bob, &0i128);
    assert_eq!(err, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn test_contribute_rejects_unknown_escrow() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let bob = Address::generate(&env);
    let err = client.try_contribute(&999u64, &bob, &1_000i128);
    assert_eq!(err, Err(Ok(Error::EscrowNotFound)));
}

#[test]
fn test_contribute_rejects_after_already_paid() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let maintainer = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);
    asset_client.mint(&bob, &10_000i128);

    client.fund(&104u64, &alice, &token_addr, &5_000i128, &1_000u64);
    client.release(&104u64, &vec![&env, (maintainer, 10_000u32)]);

    let err = client.try_contribute(&104u64, &bob, &1_000i128);
    assert_eq!(err, Err(Ok(Error::AlreadyPaid)));
}

#[test]
fn test_contribute_rejects_beyond_max_sponsors() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);

    client.fund(&105u64, &alice, &token_addr, &1_000i128, &1_000u64);

    // MAX_SPONSORS is 20; alice's `fund` call above already used slot 0, so
    // 19 more `contribute` calls exactly fill the cap.
    for _ in 0..(crate::MAX_SPONSORS - 1) {
        let extra = Address::generate(&env);
        asset_client.mint(&extra, &1_000i128);
        client.contribute(&105u64, &extra, &1_000i128);
    }
    assert_eq!(
        client.get_escrow(&105u64).contributor_count,
        crate::MAX_SPONSORS
    );

    // The 21st distinct contribution is rejected.
    let one_too_many = Address::generate(&env);
    asset_client.mint(&one_too_many, &1_000i128);
    let err = client.try_contribute(&105u64, &one_too_many, &1_000i128);
    assert_eq!(err, Err(Ok(Error::TooManySponsors)));
}

#[test]
fn test_extend_deadline_any_contributor_can_extend_not_just_the_original_funder() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);
    asset_client.mint(&bob, &10_000i128);

    env.ledger().set_timestamp(100);
    client.fund(&106u64, &alice, &token_addr, &5_000i128, &200u64);
    client.contribute(&106u64, &bob, &5_000i128);

    // Bob (the second contributor, not the original funder) extends.
    client.extend_deadline(&106u64, &bob, &500u64);
    assert_eq!(client.get_escrow(&106u64).deadline, 500u64);

    // The old deadline (200) has passed, but the extended one (500) hasn't:
    // refund must still require admin auth.
    env.ledger().set_timestamp(300);
    env.set_auths(&[]);
    let result = client.try_refund(&106u64);
    assert!(result.is_err());
}

#[test]
fn test_extend_deadline_rejects_non_contributor() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);

    client.fund(&107u64, &alice, &token_addr, &5_000i128, &1_000u64);

    // A stranger who never contributed to this escrow, even with valid
    // auth for themselves, cannot extend it.
    let stranger = Address::generate(&env);
    let err = client.try_extend_deadline(&107u64, &stranger, &2_000u64);
    assert_eq!(err, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_get_contribution_enumerates_each_contributor() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);
    asset_client.mint(&bob, &10_000i128);

    client.fund(&108u64, &alice, &token_addr, &4_000i128, &1_000u64);
    client.contribute(&108u64, &bob, &6_000i128);

    let c0 = client.get_contribution(&108u64, &0u32);
    let c1 = client.get_contribution(&108u64, &1u32);
    assert_eq!(c0.sponsor, alice);
    assert_eq!(c0.amount, 4_000i128);
    assert_eq!(c1.sponsor, bob);
    assert_eq!(c1.amount, 6_000i128);

    let err = client.try_get_contribution(&108u64, &2u32);
    assert_eq!(err, Err(Ok(Error::EscrowNotFound)));
}

#[test]
fn test_release_succeeds_in_grace_period() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    env.ledger().set_timestamp(100);
    client.fund(&200u64, &sponsor, &token_addr, &10_000_000_000i128, &200u64);

    // Pass the nominal deadline but stay within the grace period.
    env.ledger().set_timestamp(200 + crate::GRACE_PERIOD - 1);

    // Permissionless refund is still rejected.
    env.set_auths(&[]);
    let result = client.try_refund(&200u64);
    assert!(result.is_err());

    // Release still succeeds, and doesn't get front-run.
    env.mock_all_auths();
    let contributor = Address::generate(&env);
    let recipients = vec![&env, (contributor.clone(), 10_000u32)];
    client.release(&200u64, &recipients);
    assert_eq!(token_client.balance(&contributor), 9_500_000_000i128);
    assert_eq!(client.get_escrow(&200u64).status, EscrowStatus::Paid);
}

#[test]
fn test_release_loses_race_to_refund_at_grace_period_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    env.ledger().set_timestamp(100);
    client.fund(&201u64, &sponsor, &token_addr, &10_000_000_000i128, &200u64);

    // Reach the exact boundary where the permissionless path opens.
    env.ledger().set_timestamp(200 + crate::GRACE_PERIOD);

    // Refund lands first (permissionless).
    env.set_auths(&[]);
    client.refund(&201u64);
    assert_eq!(token_client.balance(&sponsor), 10_000_000_000i128);

    // The backend's subsequently-landing release call fails.
    env.mock_all_auths();
    let contributor = Address::generate(&env);
    let recipients = vec![&env, (contributor.clone(), 10_000u32)];
    let err = client.try_release(&201u64, &recipients);
    assert_eq!(err, Err(Ok(Error::AlreadyRefunded)));

    // The would-be recipient gets nothing.
    assert_eq!(token_client.balance(&contributor), 0);
}
