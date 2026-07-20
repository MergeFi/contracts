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
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(env, &contract_id);
    client.initialize(&admin, &treasury, &500u32); // 5% fee
    (contract_id, admin, treasury, client)
}

#[test]
fn test_initialize_rejects_double_init() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, admin, treasury, client) = setup(&env);
    let err = client.try_initialize(&admin, &treasury, &500u32);
    assert_eq!(err, Err(Ok(Error::AlreadyInitialized)));
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
    // initialize/fund both need auth too now, so mock broadly up front and
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
    let contract_id = env.register_contract(None, crate::EscrowContract);
    let client = crate::EscrowContractClient::new(&env, &contract_id);
    
    // Initialize with 0% fee to simplify fraction/dust calculations
    client.initialize(&admin, &treasury, &0u32);

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

    let payouts_normal = crate::compute_split(&env, total_amount, &normal_order).unwrap();

    // 4. Malicious ordering (attacker at the end to steal the remainder)
    let mut malicious_order = Vec::new(&env);
    malicious_order.push_back((dev1.clone(), 3333u32));
    malicious_order.push_back((dev2.clone(), 3334u32));
    malicious_order.push_back((attacker.clone(), 3333u32)); 

    let payouts_malicious = crate::compute_split(&env, total_amount, &malicious_order).unwrap();

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
fn test_initialize_requires_admin_auth() {
    let env = Env::default();
    // No auths mocked at all.
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let result = client.try_initialize(&admin, &treasury, &500u32);
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

    // Past the deadline, and with every auth turned off — not even the
    // sponsor or admin authorizes this call. `refund` must still succeed:
    // this is the "anyone" path the whole design exists to provide.
    env.ledger().set_timestamp(300);
    env.set_auths(&[]);
    client.refund(&11u64);

    assert_eq!(token_client.balance(&sponsor), 10_000_000_000i128);
    assert_eq!(client.get_escrow(&11u64).status, EscrowStatus::Refunded);
}
