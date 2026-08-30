#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{storage::Persistent as _, Address as _, Ledger},
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
    let oracle = Address::generate(env);
    let treasury = Address::generate(env);
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(env, &contract_id);
    client.initialize(&admin, &oracle, &treasury, &500u32, &None); // 5% fee
    (contract_id, admin, treasury, client)
}

#[test]
fn test_initialize_rejects_double_init() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, admin, treasury, client) = setup(&env);
    let oracle = Address::generate(&env);
    let err = client.try_initialize(&admin, &oracle, &treasury, &500u32, &None);
    assert_eq!(err, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_initialize_rejects_fee_bps_above_10000() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let err = client.try_initialize(&admin, &oracle, &treasury, &10_001u32, &None);
    assert_eq!(err, Err(Ok(Error::InvalidFee)));
}

#[test]
fn test_initialize_accepts_fee_bps_at_boundary_10000() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    client.initialize(&admin, &oracle, &treasury, &10_000u32, &None);
    assert_eq!(client.get_fee_bps(), 10_000u32);
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

    client.fund(
        &1u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );

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

    client.fund(
        &2u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );

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

    client.fund(&8u64, &sponsor, &token_addr, &101i128, &1_000u64, &None);

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

    client.fund(
        &3u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );

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

    client.fund(
        &4u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );
    let recipients = vec![&env, (contributor.clone(), 10_000u32)];
    client.release(&4u64, &recipients);

    let err = client.try_release(&4u64, &recipients);
    assert_eq!(err, Err(Ok(Error::AlreadyPaid)));
}

#[test]
fn test_unauthorized_release_rejected() {
    let env = Env::default();
    // initialize/fund both need auth too now, so mock broadly up front and
    // turn it off only for the specific unauthorized call under test below.
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);

    asset_client.mint(&sponsor, &10_000_000_000i128);
    client.fund(
        &5u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );

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

    client.fund(
        &6u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &200u64,
        &None,
    );

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

    client.fund(
        &7u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );
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
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(crate::EscrowContract, ());
    let client = crate::EscrowContractClient::new(&env, &contract_id);

    // Initialize with 0% fee to simplify fraction/dust calculations
    client.initialize(&admin, &oracle, &treasury, &0u32, &None);

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

    // compute_split itself no longer reads storage (issue #142: it now
    // takes fee_bps as a plain argument, see mergefi-common::split) — the
    // env.as_contract wrapper is kept here unchanged from before that move.
    let payouts_normal = env.as_contract(&contract_id, || {
        mergefi_common::compute_split(&env, total_amount, 0u32, &normal_order).unwrap()
    });

    // 4. Malicious ordering (attacker at the end to steal the remainder)
    let mut malicious_order = Vec::new(&env);
    malicious_order.push_back((dev1.clone(), 3333u32));
    malicious_order.push_back((dev2.clone(), 3334u32));
    malicious_order.push_back((attacker.clone(), 3333u32));

    let payouts_malicious = env.as_contract(&contract_id, || {
        mergefi_common::compute_split(&env, total_amount, 0u32, &malicious_order).unwrap()
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

#[test]
fn test_large_split_distributes_dust_by_largest_remainder() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(crate::EscrowContract, ());
    let client = crate::EscrowContractClient::new(&env, &contract_id);
    // 0% fee so the whole total is distributable.
    client.initialize(&admin, &oracle, &treasury, &0u32, &None);

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
        mergefi_common::compute_split(&env, total, 0u32, &recipients).unwrap()
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

// ---------------------------------------------------------------------------
// Access-control boundary matrix (#30)
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_requires_admin_auth() {
    let env = Env::default();
    // No auths mocked at all.
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let result = client.try_initialize(&admin, &oracle, &treasury, &500u32, &None);
    assert!(result.is_err());
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
    let result = client.try_fund(
        &9u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );
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
    client.fund(
        &10u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &200u64,
        &None,
    );

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
    client.fund(
        &11u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &200u64,
        &None,
    );

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
    client.fund(
        &12u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &200u64,
        &None,
    );

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
    client.fund(
        &13u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &200u64,
        &None,
    );

    client.extend_deadline(&13u64, &sponsor, &500u64);
    assert_eq!(client.get_escrow(&13u64).deadline, 500u64);

    // Old deadline (200) has now passed, but the extended one (500) hasn't:
    // refund must still require admin auth, proving the extension actually
    // re-closed the permissionless window.
    env.ledger().set_timestamp(300);
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
    client.fund(
        &14u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &200u64,
        &None,
    );

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
        &None,
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
    client.fund(&100u64, &alice, &token_addr, &3_000i128, &200u64, &None);
    client.contribute(&100u64, &bob, &7_000i128);
    client.contribute(&100u64, &carol, &1_500i128);

    let escrow = client.get_escrow(&100u64);
    assert_eq!(escrow.amount, 11_500i128);
    assert_eq!(escrow.contributor_count, 3);

    // Past the deadline + grace period: permissionless refund. GRACE_PERIOD
    // is expressed in seconds (14 days), so this must advance well past
    // `deadline` alone, matching the pattern in
    // test_refund_after_deadline_is_permissionless — advancing only to 300
    // (as a prior version of this test did) leaves `refund` still inside
    // the admin-only window, which is why `env.set_auths(&[])` below used
    // to make this call fail with an auth error unrelated to what the test
    // is actually about.
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

    client.fund(&101u64, &alice, &token_addr, &4_000i128, &1_000u64, &None);
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

    client.fund(&102u64, &alice, &token_addr, &5_000i128, &1_000u64, &None);

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

    client.fund(&103u64, &alice, &token_addr, &5_000i128, &1_000u64, &None);

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

    client.fund(&104u64, &alice, &token_addr, &5_000i128, &1_000u64, &None);
    client.release(&104u64, &vec![&env, (maintainer, 10_000u32)]);

    let err = client.try_contribute(&104u64, &bob, &1_000i128);
    assert_eq!(err, Err(Ok(Error::AlreadyPaid)));
}

#[test]
fn test_contribute_rejects_after_already_refunded() {
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
    client.fund(&106u64, &alice, &token_addr, &5_000i128, &200u64, &None);

    env.ledger().set_timestamp(200 + crate::GRACE_PERIOD);
    client.refund(&106u64);

    let err = client.try_contribute(&106u64, &bob, &1_000i128);
    assert_eq!(err, Err(Ok(Error::AlreadyRefunded)));
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

    client.fund(&105u64, &alice, &token_addr, &1_000i128, &1_000u64, &None);

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
fn test_get_max_sponsors_defaults_to_the_constant_when_not_specified() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    assert_eq!(client.get_max_sponsors(), crate::MAX_SPONSORS);
}

#[test]
fn test_initialize_accepts_a_custom_max_sponsors() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);
    client.initialize(&admin, &oracle, &treasury, &500u32, &Some(2u32));

    assert_eq!(client.get_max_sponsors(), 2u32);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);
    client.fund(&106u64, &alice, &token_addr, &1_000i128, &1_000u64, &None);

    let bob = Address::generate(&env);
    asset_client.mint(&bob, &1_000i128);
    client.contribute(&106u64, &bob, &1_000i128);

    // With max_sponsors == 2, alice's `fund` (slot 0) plus bob's
    // `contribute` (slot 1) already fill the custom cap.
    let carol = Address::generate(&env);
    asset_client.mint(&carol, &1_000i128);
    let err = client.try_contribute(&106u64, &carol, &1_000i128);
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
    client.fund(&106u64, &alice, &token_addr, &5_000i128, &200u64, &None);
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

    client.fund(&107u64, &alice, &token_addr, &5_000i128, &1_000u64, &None);

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

    client.fund(&108u64, &alice, &token_addr, &4_000i128, &1_000u64, &None);
    client.contribute(&108u64, &bob, &6_000i128);

    let c0 = client.get_contribution(&108u64, &0u32);
    let c1 = client.get_contribution(&108u64, &1u32);
    assert_eq!(c0.sponsor, alice);
    assert_eq!(c0.amount, 4_000i128);
    assert_eq!(c0.timestamp, env.ledger().timestamp());
    assert_eq!(c1.sponsor, bob);
    assert_eq!(c1.amount, 6_000i128);
    assert_eq!(c1.timestamp, env.ledger().timestamp());

    // Advance time and top up; timestamp updates to latest deposit time
    env.ledger().set_timestamp(env.ledger().timestamp() + 500);
    client.contribute(&108u64, &bob, &1_000i128);
    let c1_topup = client.get_contribution(&108u64, &1u32);
    assert_eq!(c1_topup.amount, 7_000i128);
    assert_eq!(c1_topup.timestamp, env.ledger().timestamp());

    let err = client.try_get_contribution(&108u64, &2u32);
    assert_eq!(err, Err(Ok(Error::ContributionNotFound)));
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
    client.fund(
        &200u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &200u64,
        &None,
    );

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
    client.fund(
        &201u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &200u64,
        &None,
    );

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

#[test]
fn test_pause_blocks_mutating_entrypoints_but_allows_expired_refund() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &20_000i128);
    let contributor = Address::generate(&env);

    env.ledger().set_timestamp(100);
    client.fund(&300u64, &sponsor, &token_addr, &10_000i128, &200u64, &None);
    client.pause();
    assert!(client.is_paused_view());

    let err = client.try_fund(&301u64, &sponsor, &token_addr, &1_000i128, &200u64, &None);
    assert_eq!(err, Err(Ok(Error::ContractPaused)));

    let err = client.try_contribute(&300u64, &sponsor, &1_000i128);
    assert_eq!(err, Err(Ok(Error::ContractPaused)));

    let err = client.try_extend_deadline(&300u64, &sponsor, &500u64);
    assert_eq!(err, Err(Ok(Error::ContractPaused)));

    let err = client.try_release(&300u64, &vec![&env, (contributor, 10_000u32)]);
    assert_eq!(err, Err(Ok(Error::ContractPaused)));

    env.ledger().set_timestamp(200 + crate::GRACE_PERIOD);
    env.set_auths(&[]);
    client.refund(&300u64);
    assert_eq!(token_client.balance(&sponsor), 20_000i128);
    assert_eq!(client.get_escrow(&300u64).status, EscrowStatus::Refunded);
}

#[test]
fn test_unpause_restores_funding() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000i128);

    client.pause();
    assert!(client.is_paused_view());
    client.unpause();
    assert!(!client.is_paused_view());

    client.fund(
        &302u64,
        &sponsor,
        &token_addr,
        &10_000i128,
        &1_000u64,
        &None,
    );
    assert_eq!(client.get_escrow(&302u64).status, EscrowStatus::Funded);
}

#[test]
fn test_oracle_is_stored_and_rotatable() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    client.initialize(&admin, &oracle, &treasury, &500u32, &None);
    assert_eq!(client.get_oracle(), oracle);
    assert_eq!(client.get_version(), 1);

    let new_oracle = Address::generate(&env);
    client.set_oracle(&new_oracle);
    assert_eq!(client.get_oracle(), new_oracle);
}

// ── extend_deadline / keep_alive TTL scaling (#56) ─────────────────────────
//
// extend_ttl's flat ~29-day (500_000-ledger) bump applied regardless of how
// far `extend_deadline` pushed the deadline out — a far-future deadline
// bought no more actual on-chain survivability than a near-future one. These
// tests exercise the fix: TTL now scales toward the deadline, capped at
// Soroban's own real ceiling.

fn escrow_ttl(env: &Env, contract_id: &Address, issue_id: u64) -> u32 {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Escrow(issue_id))
    })
}

#[test]
fn test_extend_deadline_scales_ttl_proportionally_for_a_moderately_far_future_deadline() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    client.fund(
        &301u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );

    // 90 days out — comfortably under the network's own ~1-year ceiling, so
    // this exercises the proportional path, not the cap.
    let ninety_days_secs: u64 = 90 * 24 * 60 * 60;
    client.extend_deadline(&301u64, &sponsor, &ninety_days_secs);

    let expected_ledgers = (ninety_days_secs + GRACE_PERIOD) / 5;
    assert_eq!(
        escrow_ttl(&env, &contract_id, 301u64),
        expected_ledgers as u32
    );
    // Sanity check against the old, now-wrong expectation: a 90-day
    // deadline must buy noticeably more than the flat 500_000-ledger bump.
    assert!(expected_ledgers > 500_000);
}

#[test]
fn test_extend_deadline_caps_ttl_at_the_network_max_for_a_very_far_future_deadline() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    client.fund(
        &302u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );

    // 3 years out — the naive proportional ledger count for this would
    // exceed what Soroban actually allows a single persistent entry to
    // survive to. The record must still get *something* (the maximum this
    // call can grant), not silently fall back to the flat 29-day bump.
    let three_years_secs: u64 = 3 * 365 * 24 * 60 * 60;
    client.extend_deadline(&302u64, &sponsor, &three_years_secs);

    let max_extend_to = env.ledger().max_live_until_ledger() - env.ledger().sequence();
    assert_eq!(escrow_ttl(&env, &contract_id, 302u64), max_extend_to);
    assert!(max_extend_to > 500_000);
}

#[test]
fn test_extend_deadline_never_extends_less_than_the_existing_flat_baseline() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    client.fund(
        &303u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );

    // Only a few days beyond the current deadline — the proportional target
    // is far smaller than the flat 500_000-ledger baseline every other TTL
    // call site still gets. Must not regress below it.
    client.extend_deadline(&303u64, &sponsor, &2_000u64);

    assert_eq!(escrow_ttl(&env, &contract_id, 303u64), 500_000);
}

#[test]
fn test_keep_alive_refreshes_ttl_without_changing_deadline_or_status() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    let far_future_deadline: u64 = 200 * 24 * 60 * 60;
    client.fund(
        &304u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &far_future_deadline,
        &None,
    );

    let before = client.get_escrow(&304u64);

    // Anyone can call this — no auth mocked out or required for it, unlike
    // extend_deadline's contributor-only require_auth.
    client.keep_alive(&304u64);

    let after = client.get_escrow(&304u64);
    assert_eq!(
        before, after,
        "keep_alive must not change deadline or status"
    );

    let expected_ledgers = (far_future_deadline + GRACE_PERIOD) / 5;
    assert_eq!(
        escrow_ttl(&env, &contract_id, 304u64),
        expected_ledgers as u32
    );
}

#[test]
fn test_keep_alive_rejects_nonexistent_escrow() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let err = client.try_keep_alive(&999u64);
    assert_eq!(err, Err(Ok(Error::EscrowNotFound)));
}

// ── target amount (issue #144) ────────────────────────────────────────────

#[test]
fn test_fund_stores_optional_target() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    client.fund(
        &400u64,
        &sponsor,
        &token_addr,
        &3_000i128,
        &1_000u64,
        &Some(500i128),
    );

    let escrow = client.get_escrow(&400u64);
    assert_eq!(escrow.target, Some(500i128));
}

#[test]
fn test_fund_defaults_target_to_none() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    client.fund(&401u64, &sponsor, &token_addr, &3_000i128, &1_000u64, &None);

    let escrow = client.get_escrow(&401u64);
    assert_eq!(escrow.target, None);
}

#[test]
fn test_fund_rejects_non_positive_target() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    let err = client.try_fund(
        &402u64,
        &sponsor,
        &token_addr,
        &3_000i128,
        &1_000u64,
        &Some(0i128),
    );
    assert_eq!(err, Err(Ok(Error::InvalidTarget)));

    let err = client.try_fund(
        &402u64,
        &sponsor,
        &token_addr,
        &3_000i128,
        &1_000u64,
        &Some(-1i128),
    );
    assert_eq!(err, Err(Ok(Error::InvalidTarget)));
}

#[test]
fn test_contribute_past_target_is_not_blocked() {
    // target is purely informational — contribute() must never check it.
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    let sponsor2 = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);
    asset_client.mint(&sponsor2, &10_000_000_000i128);

    client.fund(
        &403u64,
        &sponsor,
        &token_addr,
        &1_000i128,
        &1_000u64,
        &Some(1_000i128),
    );
    // Already at target; contribute() should still succeed past it.
    client.contribute(&403u64, &sponsor2, &500i128);

    let escrow = client.get_escrow(&403u64);
    assert_eq!(escrow.amount, 1_500i128);
    assert_eq!(escrow.target, Some(1_000i128));
}

// ── batch contribution enumeration (issue #145) ───────────────────────────

#[test]
fn test_get_contributions_returns_full_ledger_in_order() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _) = create_token(&env, &token_admin);
    let sponsor1 = Address::generate(&env);
    let sponsor2 = Address::generate(&env);
    let sponsor3 = Address::generate(&env);
    asset_client.mint(&sponsor1, &10_000_000_000i128);
    asset_client.mint(&sponsor2, &10_000_000_000i128);
    asset_client.mint(&sponsor3, &10_000_000_000i128);

    client.fund(
        &500u64,
        &sponsor1,
        &token_addr,
        &1_000i128,
        &1_000u64,
        &None,
    );
    client.contribute(&500u64, &sponsor2, &2_000i128);
    client.contribute(&500u64, &sponsor3, &3_000i128);

    let contributions = client.get_contributions(&500u64);
    assert_eq!(contributions.len(), 3);
    assert_eq!(contributions.get(0).unwrap().sponsor, sponsor1);
    assert_eq!(contributions.get(0).unwrap().amount, 1_000i128);
    assert_eq!(contributions.get(1).unwrap().sponsor, sponsor2);
    assert_eq!(contributions.get(1).unwrap().amount, 2_000i128);
    assert_eq!(contributions.get(2).unwrap().sponsor, sponsor3);
    assert_eq!(contributions.get(2).unwrap().amount, 3_000i128);

    // Matches what 3 individual get_contribution calls would return.
    for i in 0..3u32 {
        assert_eq!(
            contributions.get(i).unwrap(),
            client.get_contribution(&500u64, &i)
        );
    }
}

#[test]
fn test_get_contributions_rejects_nonexistent_escrow() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let err = client.try_get_contributions(&999u64);
    assert_eq!(err, Err(Ok(Error::EscrowNotFound)));
}

#[test]
fn test_get_contributions_single_sponsor() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    client.fund(&501u64, &sponsor, &token_addr, &1_000i128, &1_000u64, &None);

    let contributions = client.get_contributions(&501u64);
    assert_eq!(contributions.len(), 1);
    assert_eq!(contributions.get(0).unwrap().sponsor, sponsor);
}

// ── Re-funding after terminal state (#41) ──────────────────────────────────

#[test]
fn test_fund_allows_reuse_after_refund() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &20_000_000_000i128);

    env.ledger().set_timestamp(100);
    client.fund(
        &600u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &200u64,
        &None,
    );

    // Refund after deadline.
    env.ledger().set_timestamp(200 + crate::GRACE_PERIOD);
    env.set_auths(&[]);
    client.refund(&600u64);
    assert_eq!(client.get_escrow(&600u64).status, EscrowStatus::Refunded);
    assert_eq!(token_client.balance(&sponsor), 20_000_000_000i128);

    // Re-fund the same issue_id with fresh arguments.
    env.mock_all_auths();
    client.fund(
        &600u64,
        &sponsor,
        &token_addr,
        &5_000_000_000i128,
        &500u64,
        &None,
    );

    let escrow = client.get_escrow(&600u64);
    assert_eq!(escrow.status, EscrowStatus::Funded);
    assert_eq!(escrow.amount, 5_000_000_000i128);
    assert_eq!(escrow.deadline, 500u64);
    assert_eq!(escrow.contributor_count, 1);
}

#[test]
fn test_fund_allows_reuse_after_paid() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &20_000_000_000i128);
    let contributor = Address::generate(&env);

    client.fund(
        &601u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );
    client.release(&601u64, &vec![&env, (contributor.clone(), 10_000u32)]);
    assert_eq!(client.get_escrow(&601u64).status, EscrowStatus::Paid);

    // Re-fund the same issue_id.
    client.fund(
        &601u64,
        &sponsor,
        &token_addr,
        &5_000_000_000i128,
        &2_000u64,
        &None,
    );

    let escrow = client.get_escrow(&601u64);
    assert_eq!(escrow.status, EscrowStatus::Funded);
    assert_eq!(escrow.amount, 5_000_000_000i128);
}

#[test]
fn test_fund_still_rejects_reuse_of_funded_escrow() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &20_000_000_000i128);

    client.fund(
        &602u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );

    // Still Funded — must be rejected.
    let err = client.try_fund(
        &602u64,
        &sponsor,
        &token_addr,
        &5_000_000_000i128,
        &2_000u64,
        &None,
    );
    assert_eq!(err, Err(Ok(Error::AlreadyFunded)));
}

#[contract]
pub struct MockPanicToken;

#[contractimpl]
impl MockPanicToken {
    pub fn transfer(env: Env, _from: Address, to: Address, _amount: i128) {
        let blocked_key = soroban_sdk::Symbol::new(&env, "blocked");
        if env.storage().instance().has(&blocked_key) {
            let blocked: Address = env.storage().instance().get(&blocked_key).unwrap();
            if to == blocked {
                panic!("Frozen/unauthorized trustline recipient");
            }
        }
    }

    pub fn set_blocked(env: Env, blocked: Address) {
        let blocked_key = soroban_sdk::Symbol::new(&env, "blocked");
        env.storage().instance().set(&blocked_key, &blocked);
    }
}

#[test]
fn test_release_all_or_nothing_revert_with_blocked_recipient() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_addr = env.register(MockPanicToken, ());
    let panic_client = MockPanicTokenClient::new(&env, &token_addr);

    let sponsor = Address::generate(&env);
    let dev1 = Address::generate(&env);
    let dev2 = Address::generate(&env);
    let blocked_dev = Address::generate(&env);

    panic_client.set_blocked(&blocked_dev);

    // Fund the escrow
    client.fund(&700u64, &sponsor, &token_addr, &10_000i128, &1_000u64, &None);

    // Release to a team split where one recipient is blocked
    let recipients = vec![
        &env,
        (dev1.clone(), 4_000u32),
        (dev2.clone(), 4_000u32),
        (blocked_dev.clone(), 2_000u32),
    ];

    // The release call must revert (fail) due to the blocked recipient
    let result = client.try_release(&700u64, &recipients);
    assert!(result.is_err(), "Expected release to revert when one recipient is blocked");

    // The escrow status must remain Funded (all-or-nothing revert)
    let escrow = client.get_escrow(&700u64);
    assert_eq!(escrow.status, EscrowStatus::Funded);
}

// ---------------------------------------------------------------------------
// State-machine model: EscrowStatus transitions (#26)
// ---------------------------------------------------------------------------
//
// Valid transitions (all enforced by match/if guards in lib.rs):
//
//   Funded  ──release──>  Paid
//   Funded  ──refund───>  Refunded
//   Paid    ──fund()───>  Funded   (re-funding after terminal state)
//   Refunded──fund()───>  Funded   (re-funding after terminal state)
//
// Invalid transitions (must be rejected):
//   Funded  ──release──>  Refunded (impossible)
//   Funded  ──refund───>  Paid     (impossible)
//   Paid    ──release──>  *        (AlreadyPaid)
//   Paid    ──refund───>  *        (AlreadyPaid)
//   Refunded──release──>  *        (AlreadyRefunded)
//   Refunded──refund───>  *        (AlreadyRefunded)

/// Exhaustive enumeration of all valid EscrowStatus transitions.
/// Each entry: (initial_status, operation, expected_result).
/// This serves as the formal state-machine model for EscrowStatus.

#[test]
fn test_state_machine_funded_to_paid_via_release() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000i128);

    client.fund(&800u64, &sponsor, &token_addr, &10_000i128, &1_000u64, &None);
    assert_eq!(client.get_escrow(&800u64).status, EscrowStatus::Funded);

    let maintainer = Address::generate(&env);
    client.release(&800u64, &vec![&env, (maintainer, 10_000u32)]);
    assert_eq!(client.get_escrow(&800u64).status, EscrowStatus::Paid);
}

#[test]
fn test_state_machine_funded_to_refunded_via_refund() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000i128);

    client.fund(&801u64, &sponsor, &token_addr, &10_000i128, &1_000u64, &None);
    assert_eq!(client.get_escrow(&801u64).status, EscrowStatus::Funded);

    client.refund(&801u64);
    assert_eq!(client.get_escrow(&801u64).status, EscrowStatus::Refunded);
}

#[test]
fn test_state_machine_paid_allows_refund_rejection() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000i128);

    client.fund(&810u64, &sponsor, &token_addr, &10_000i128, &1_000u64, &None);
    let maintainer = Address::generate(&env);
    client.release(&810u64, &vec![&env, (maintainer, 10_000u32)]);
    assert_eq!(client.get_escrow(&810u64).status, EscrowStatus::Paid);

    // Paid -> refund must be rejected
    let err = client.try_refund(&810u64);
    assert_eq!(err, Err(Ok(Error::AlreadyPaid)));
}

#[test]
fn test_state_machine_refunded_allows_release_rejection() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000i128);

    client.fund(&804u64, &sponsor, &token_addr, &10_000i128, &1_000u64, &None);
    client.refund(&804u64);
    assert_eq!(client.get_escrow(&804u64).status, EscrowStatus::Refunded);

    // Refunded -> release must be rejected
    let maintainer = Address::generate(&env);
    let err = client.try_release(&804u64, &vec![&env, (maintainer, 10_000u32)]);
    assert_eq!(err, Err(Ok(Error::AlreadyRefunded)));
}

#[test]
fn test_state_machine_refunded_allows_double_refund_rejection() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000i128);

    client.fund(&805u64, &sponsor, &token_addr, &10_000i128, &1_000u64, &None);
    client.refund(&805u64);

    // Refunded -> refund must be rejected
    let err = client.try_refund(&805u64);
    assert_eq!(err, Err(Ok(Error::AlreadyRefunded)));
}

#[test]
fn test_state_machine_paid_refunded_allow_refund_via_fund() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &30_000i128);

    // Paid -> fund (re-fund) -> Funded
    client.fund(&806u64, &sponsor, &token_addr, &10_000i128, &1_000u64, &None);
    let maintainer = Address::generate(&env);
    client.release(&806u64, &vec![&env, (maintainer, 10_000u32)]);
    assert_eq!(client.get_escrow(&806u64).status, EscrowStatus::Paid);

    client.fund(&806u64, &sponsor, &token_addr, &10_000i128, &2_000u64, &None);
    assert_eq!(client.get_escrow(&806u64).status, EscrowStatus::Funded);

    // Refunded -> fund (re-fund) -> Funded
    client.fund(&807u64, &sponsor, &token_addr, &10_000i128, &1_000u64, &None);
    client.refund(&807u64);
    assert_eq!(client.get_escrow(&807u64).status, EscrowStatus::Refunded);

    client.fund(&807u64, &sponsor, &token_addr, &10_000i128, &2_000u64, &None);
    assert_eq!(client.get_escrow(&807u64).status, EscrowStatus::Funded);
}
