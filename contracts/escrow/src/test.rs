#![cfg(test)]

extern crate std;

use super::*;
use mergefi_common::MAX_SPONSORS_LIMIT;
use soroban_sdk::{
    testutils::{storage::Persistent as _, Address as _, Events as _, Ledger},
    token, vec, Address, Env, Event,
};

/// Hash of the empty Wasm module that Soroban's test host associates with
/// contracts registered through their Rust function set. Reusing it lets the
/// upgrade tests call `update_current_contract_wasm` without a compiled Wasm
/// artifact, while subsequent calls continue to dispatch to the Rust mocks.
fn existing_wasm_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(
        env,
        &[
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55,
        ],
    )
}

/// Runs `f` and asserts it panics with the host's `Auth/InvalidAction` error.
/// The typed `try_*` client methods collapse host errors into
/// `InvokeError::Abort`, so catching the panic is the only way to assert the
/// specific authorization error code.
fn assert_host_auth_error(f: impl FnOnce()) {
    let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .expect_err("expected the call to fail authorization");
    let message = if let Some(s) = err.downcast_ref::<&str>() {
        *s
    } else if let Some(s) = err.downcast_ref::<std::string::String>() {
        s.as_str()
    } else {
        panic!("authorization panic payload was not a string");
    };
    assert!(
        message.contains("Error(Auth, InvalidAction)"),
        "expected host auth error, got: {message}"
    );
}

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

fn setup(env: &Env) -> (Address, Address, Address, EscrowContractClient<'_>) {
    let admin = Address::generate(env);
    let oracle = Address::generate(env);
    let treasury = Address::generate(env);
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(env, &contract_id);
    client.initialize(&admin, &oracle, &treasury, &500u32, &None, &None); // 5% fee
    (contract_id, admin, treasury, client)
}

#[test]
fn test_initialize_rejects_double_init() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, admin, treasury, client) = setup(&env);
    let oracle = Address::generate(&env);
    let err = client.try_initialize(&admin, &oracle, &treasury, &500u32, &None, &None);
    assert_eq!(err, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_getters_return_not_initialized_before_initialize() {
    let env = Env::default();
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    assert_eq!(client.try_get_admin(), Err(Ok(Error::NotInitialized)));
    assert_eq!(client.try_get_oracle(), Err(Ok(Error::NotInitialized)));
    assert_eq!(client.try_get_treasury(), Err(Ok(Error::NotInitialized)));
    assert_eq!(client.try_get_fee_bps(), Err(Ok(Error::NotInitialized)));
    assert_eq!(
        client.try_get_max_sponsors(),
        Err(Ok(Error::NotInitialized))
    );
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

    let err = client.try_initialize(&admin, &oracle, &treasury, &10_001u32, &None, &None);
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

    client.initialize(&admin, &oracle, &treasury, &10_000u32, &None, &None);
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
fn test_escrow_lifecycle_events_include_issue_and_payout_details() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _, treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000i128);
    let contributor = Address::generate(&env);
    asset_client.mint(&contributor, &500i128);
    let recipient = Address::generate(&env);

    client.fund(
        &501u64,
        &sponsor,
        &token_addr,
        &1_000i128,
        &1_000u64,
        &Some(2_000i128),
    );
    assert_event(
        &env,
        &contract_id,
        EscrowFunded {
            issue_id: 501,
            sponsor: sponsor.clone(),
            token: token_addr.clone(),
            amount: 1_000,
            deadline: 1_000,
            target: Some(2_000),
        },
    );

    client.contribute(&501u64, &contributor, &500i128);
    assert_event(
        &env,
        &contract_id,
        EscrowContributed {
            issue_id: 501,
            sponsor: contributor,
            amount: 500,
            total_amount: 1_500,
        },
    );

    let recipients = vec![&env, (recipient.clone(), 10_000u32)];
    client.release(&501u64, &recipients);
    assert_event(
        &env,
        &contract_id,
        EscrowPaidTo {
            issue_id: 501,
            recipient,
            amount: 1_425,
        },
    );
    assert_event(
        &env,
        &contract_id,
        EscrowReleased {
            issue_id: 501,
            amount: 1_500,
            fee: 75,
            treasury,
        },
    );

    let deadline = env.ledger().timestamp() + 1_000;
    client.fund(&502u64, &sponsor, &token_addr, &1_000i128, &deadline, &None);
    client.extend_deadline(&502u64, &sponsor, &(deadline + 1_000));
    assert_event(
        &env,
        &contract_id,
        EscrowDeadlineExtended {
            issue_id: 502,
            old_deadline: deadline,
            new_deadline: deadline + 1_000,
        },
    );
    client.refund(&502u64);
    assert_event(
        &env,
        &contract_id,
        EscrowRefundPaid {
            issue_id: 502,
            sponsor,
            amount: 1_000,
        },
    );
    assert_event(
        &env,
        &contract_id,
        EscrowRefunded {
            issue_id: 502,
            amount: 1_000,
        },
    );
}

#[test]
fn test_administrative_actions_emit_events() {
    let env = Env::default();
    env.mock_all_auths();
    let recovery = Address::generate(&env);
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    client.initialize(
        &admin,
        &oracle,
        &treasury,
        &500u32,
        &None,
        &Some(recovery.clone()),
    );
    assert_event(
        &env,
        &contract_id,
        ContractInitialized {
            admin: admin.clone(),
            oracle: oracle.clone(),
            treasury: treasury.clone(),
            fee_bps: 500,
            max_sponsors: MAX_SPONSORS,
            recovery: Some(recovery.clone()),
        },
    );

    client.pause();
    assert_event(
        &env,
        &contract_id,
        ContractPaused {
            admin: admin.clone(),
        },
    );
    client.unpause();
    assert_event(
        &env,
        &contract_id,
        ContractUnpaused {
            admin: admin.clone(),
        },
    );

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
    client.set_fee_bps(&1_000u32);
    assert_event(
        &env,
        &contract_id,
        FeeBpsChanged {
            old_fee_bps: 500,
            new_fee_bps: 1_000,
        },
    );

    // `upgrade` no longer emits `ContractUpgraded`; it is covered by the
    // dedicated upgrade tests below.
    let wasm_hash = existing_wasm_hash(&env);
    client.upgrade(&wasm_hash);
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
fn test_release_with_zero_fee_pays_full_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);
    client.initialize(&admin, &oracle, &treasury, &0u32, &None, &None);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    client.fund(
        &1u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );

    let contributor = Address::generate(&env);
    let recipients = vec![&env, (contributor.clone(), 10_000u32)];
    client.release(&1u64, &recipients);

    assert_eq!(token_client.balance(&contributor), 10_000_000_000i128);
    assert_eq!(token_client.balance(&treasury), 0i128);
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
fn test_release_accepts_targeted_oracle_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _admin, treasury, client) = setup(&env);
    let oracle = client.get_oracle();

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000i128);
    client.fund(
        &500u64,
        &sponsor,
        &token_addr,
        &10_000i128,
        &1_000u64,
        &None,
    );

    let recipient = Address::generate(&env);
    let recipients = vec![&env, (recipient.clone(), 10_000u32)];

    // Only the oracle's authorization is mocked for this release call.
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &oracle,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "release",
            args: soroban_sdk::IntoVal::into_val(&(500u64, recipients.clone()), &env),
            sub_invokes: &[],
        },
    }]);

    client.release(&500u64, &recipients);

    assert_eq!(client.get_escrow(&500u64).status, EscrowStatus::Paid);
    assert_eq!(token_client.balance(&recipient), 9_500i128);
    assert_eq!(token_client.balance(&treasury), 500i128);
}

#[test]
fn test_release_rejects_non_oracle_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000i128);
    client.fund(
        &501u64,
        &sponsor,
        &token_addr,
        &10_000i128,
        &1_000u64,
        &None,
    );

    let attacker = Address::generate(&env);
    let recipient = Address::generate(&env);
    let recipients = vec![&env, (recipient, 10_000u32)];

    // Mock a non-oracle address as the signer: the oracle's `require_auth`
    // must still fail with a host authorization error.
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &attacker,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "release",
            args: soroban_sdk::IntoVal::into_val(&(501u64, recipients.clone()), &env),
            sub_invokes: &[],
        },
    }]);

    assert_host_auth_error(|| client.release(&501u64, &recipients));
    assert_eq!(client.get_escrow(&501u64).status, EscrowStatus::Funded);
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
    client.initialize(&admin, &oracle, &treasury, &0u32, &None, &None);

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
    client.initialize(&admin, &oracle, &treasury, &0u32, &None, &None);

    // The maximum 20 recipients: 19 with alternating 160/170 bps, the last
    // one receiving the leftover of 10000. All 170-bps recipients share an
    // identical remainder, so most of the dust has to be resolved by the
    // address-based tie-break, exercising both the O(n log n) sort and the
    // tie-break at scale.
    let mut recipients = Vec::new(&env);
    let mut total_bps: u32 = 0;
    for i in 0..19u32 {
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

    let result = client.try_initialize(&admin, &oracle, &treasury, &500u32, &None, &None);
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
fn test_fund_accepts_targeted_sponsor_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &1_000i128);

    let issue_id = 520u64;
    let amount = 1_000i128;
    let deadline = 1_000u64;
    let params = (
        issue_id,
        sponsor.clone(),
        token_addr.clone(),
        amount,
        deadline,
        Option::<i128>::None,
    );
    let transfer_params = (sponsor.clone(), contract_id.clone(), amount);

    // Only the sponsor's authorization is mocked for this call (including the
    // token transfer it authorizes as a sub-invocation).
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &sponsor,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "fund",
            args: soroban_sdk::IntoVal::into_val(&params, &env),
            sub_invokes: &[soroban_sdk::testutils::MockAuthInvoke {
                contract: &token_addr,
                fn_name: "transfer",
                args: soroban_sdk::IntoVal::into_val(&transfer_params, &env),
                sub_invokes: &[],
            }],
        },
    }]);

    client.fund(&issue_id, &sponsor, &token_addr, &amount, &deadline, &None);

    assert_eq!(client.get_escrow(&issue_id).amount, amount);
    assert_eq!(token_client.balance(&sponsor), 0);
}

#[test]
fn test_fund_rejects_non_sponsor_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &1_000i128);

    let attacker = Address::generate(&env);
    let issue_id = 521u64;
    let amount = 1_000i128;
    let deadline = 1_000u64;
    let params = (
        issue_id,
        sponsor.clone(),
        token_addr.clone(),
        amount,
        deadline,
        Option::<i128>::None,
    );
    let transfer_params = (sponsor.clone(), contract_id.clone(), amount);

    // Mock a non-sponsor address as the signer: the sponsor's `require_auth`
    // must still fail with a host authorization error.
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &attacker,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "fund",
            args: soroban_sdk::IntoVal::into_val(&params, &env),
            sub_invokes: &[soroban_sdk::testutils::MockAuthInvoke {
                contract: &token_addr,
                fn_name: "transfer",
                args: soroban_sdk::IntoVal::into_val(&transfer_params, &env),
                sub_invokes: &[],
            }],
        },
    }]);

    assert_host_auth_error(|| {
        client.fund(&issue_id, &sponsor, &token_addr, &amount, &deadline, &None)
    });
    assert_eq!(
        client.try_get_escrow(&issue_id),
        Err(Ok(Error::EscrowNotFound))
    );
    assert_eq!(token_client.balance(&sponsor), 1_000i128);
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
fn test_refund_before_deadline_accepts_targeted_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000i128);

    env.ledger().set_timestamp(100);
    client.fund(&510u64, &sponsor, &token_addr, &10_000i128, &200u64, &None);

    // The deadline has not passed, so only the admin may refund early.
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "refund",
            args: soroban_sdk::IntoVal::into_val(&(510u64,), &env),
            sub_invokes: &[],
        },
    }]);

    client.refund(&510u64);

    assert_eq!(client.get_escrow(&510u64).status, EscrowStatus::Refunded);
    assert_eq!(token_client.balance(&sponsor), 10_000i128);
}

#[test]
fn test_refund_before_deadline_rejects_non_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000i128);

    env.ledger().set_timestamp(100);
    client.fund(&511u64, &sponsor, &token_addr, &10_000i128, &200u64, &None);

    let attacker = Address::generate(&env);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &attacker,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "refund",
            args: soroban_sdk::IntoVal::into_val(&(511u64,), &env),
            sub_invokes: &[],
        },
    }]);

    assert_host_auth_error(|| client.refund(&511u64));
    assert_eq!(client.get_escrow(&511u64).status, EscrowStatus::Funded);
    assert_eq!(token_client.balance(&sponsor), 0);
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

#[test]
fn test_extend_deadline_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    env.ledger().set_timestamp(100);
    client.fund(
        &16u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &200u64,
        &None,
    );

    // A non-sponsor (even a contributor) cannot extend the deadline.
    let non_sponsor = Address::generate(&env);
    let err = client.try_extend_deadline(&16u64, &non_sponsor, &500u64);
    assert_eq!(err, Err(Ok(Error::Unauthorized)));
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
fn test_contribute_accepts_targeted_sponsor_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);
    asset_client.mint(&bob, &10_000i128);

    let issue_id = 522u64;
    client.fund(&issue_id, &alice, &token_addr, &5_000i128, &1_000u64, &None);

    let amount = 5_000i128;
    let params = (issue_id, bob.clone(), amount);
    let transfer_params = (bob.clone(), contract_id.clone(), amount);

    // Only bob's authorization is mocked for this contribution (including the
    // token transfer it authorizes as a sub-invocation).
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &bob,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "contribute",
            args: soroban_sdk::IntoVal::into_val(&params, &env),
            sub_invokes: &[soroban_sdk::testutils::MockAuthInvoke {
                contract: &token_addr,
                fn_name: "transfer",
                args: soroban_sdk::IntoVal::into_val(&transfer_params, &env),
                sub_invokes: &[],
            }],
        },
    }]);

    client.contribute(&issue_id, &bob, &amount);

    let escrow = client.get_escrow(&issue_id);
    assert_eq!(escrow.amount, 10_000i128);
    assert_eq!(escrow.contributor_count, 2);
    assert_eq!(token_client.balance(&bob), 5_000i128);
}

#[test]
fn test_contribute_rejects_non_sponsor_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let attacker = Address::generate(&env);
    asset_client.mint(&alice, &10_000i128);
    asset_client.mint(&bob, &10_000i128);

    let issue_id = 523u64;
    client.fund(&issue_id, &alice, &token_addr, &5_000i128, &1_000u64, &None);

    let amount = 5_000i128;
    let params = (issue_id, bob.clone(), amount);
    let transfer_params = (bob.clone(), contract_id.clone(), amount);

    // Mock a non-sponsor address as the signer: bob's `require_auth` must
    // still fail with a host authorization error.
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &attacker,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "contribute",
            args: soroban_sdk::IntoVal::into_val(&params, &env),
            sub_invokes: &[soroban_sdk::testutils::MockAuthInvoke {
                contract: &token_addr,
                fn_name: "transfer",
                args: soroban_sdk::IntoVal::into_val(&transfer_params, &env),
                sub_invokes: &[],
            }],
        },
    }]);

    assert_host_auth_error(|| client.contribute(&issue_id, &bob, &amount));

    let escrow = client.get_escrow(&issue_id);
    assert_eq!(escrow.amount, 5_000i128);
    assert_eq!(escrow.contributor_count, 1);
    assert_eq!(token_client.balance(&bob), 10_000i128);
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

// ── Regression #473 (closed #347) ─────────────────────────────────────────
// An existing sponsor topping up when the escrow is already at the
// MAX_SPONSORS cap must succeed (it does not add a new slot).
#[test]
fn test_existing_sponsor_can_top_up_when_at_max_sponsors_cap() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);

    let alice = Address::generate(&env);
    asset_client.mint(&alice, &20_000i128);

    // Alice funds: this occupies slot 0.
    client.fund(&200u64, &alice, &token_addr, &1_000i128, &1_000u64, &None);

    // Fill the remaining MAX_SPONSORS-1 slots with distinct sponsors.
    for _ in 0..(crate::MAX_SPONSORS - 1) {
        let extra = Address::generate(&env);
        asset_client.mint(&extra, &1_000i128);
        client.contribute(&200u64, &extra, &1_000i128);
    }
    assert_eq!(
        client.get_escrow(&200u64).contributor_count,
        crate::MAX_SPONSORS,
        "escrow must be at the cap before the top-up"
    );

    // Alice (existing sponsor, slot 0) tops up — must succeed even though the
    // cap is full, because she does not consume a new slot.
    let balance_before = client.get_escrow(&200u64).amount;
    client.contribute(&200u64, &alice, &1_000i128);
    let balance_after = client.get_escrow(&200u64).amount;
    assert_eq!(balance_after - balance_before, 1_000i128);

    // contributor_count must not increase beyond the cap.
    assert_eq!(
        client.get_escrow(&200u64).contributor_count,
        crate::MAX_SPONSORS
    );
}

#[test]
fn test_get_max_sponsors_defaults_to_the_constant_when_not_specified() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    assert_eq!(client.get_max_sponsors(), crate::MAX_SPONSORS);
    assert_eq!(client.get_grace_period(), crate::GRACE_PERIOD);
    assert_eq!(client.get_bps_denominator(), BPS_DENOMINATOR);
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
    client.initialize(&admin, &oracle, &treasury, &500u32, &Some(2u32), &None);

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
fn test_initialize_caps_max_sponsors_at_shared_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let err = client.try_initialize(
        &admin,
        &oracle,
        &treasury,
        &500u32,
        &Some(MAX_SPONSORS_LIMIT + 1),
        &None,
    );
    assert_eq!(err, Err(Ok(Error::InvalidMaxSponsors)));

    client.initialize(
        &admin,
        &oracle,
        &treasury,
        &500u32,
        &Some(MAX_SPONSORS_LIMIT),
        &None,
    );
    assert_eq!(client.get_max_sponsors(), MAX_SPONSORS_LIMIT);
}

#[test]
fn test_extend_deadline_rejects_later_contributor_only_original_sponsor_can_extend() {
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

    // Bob (a later contributor, not the original funder) cannot extend.
    let err = client.try_extend_deadline(&106u64, &bob, &500u64);
    assert_eq!(err, Err(Ok(Error::Unauthorized)));
    assert_eq!(client.get_escrow(&106u64).deadline, 200u64);

    // Alice (the original sponsor) still can.
    client.extend_deadline(&106u64, &alice, &500u64);
    assert_eq!(client.get_escrow(&106u64).deadline, 500u64);
}

#[test]
fn test_extend_deadline_dust_contributor_cannot_lock_out_permissionless_refund() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    let attacker = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);
    asset_client.mint(&attacker, &1i128);

    env.ledger().set_timestamp(100);
    client.fund(
        &108u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &200u64,
        &None,
    );

    // Attacker contributes 1 stroop and tries to push the deadline out
    // forever.
    client.contribute(&108u64, &attacker, &1i128);
    let err = client.try_extend_deadline(&108u64, &attacker, &(u64::MAX - crate::GRACE_PERIOD));
    assert_eq!(err, Err(Ok(Error::Unauthorized)));
    assert_eq!(client.get_escrow(&108u64).deadline, 200u64);

    // The permissionless refund still opens on schedule.
    env.ledger().set_timestamp(200 + crate::GRACE_PERIOD);
    env.set_auths(&[]);
    client.refund(&108u64);
    assert_eq!(token_client.balance(&sponsor), 10_000_000_000i128);
    assert_eq!(token_client.balance(&attacker), 1i128);
}

#[test]
fn test_extend_deadline_rejects_deadline_beyond_max_extension() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000i128);

    env.ledger().set_timestamp(100);
    client.fund(&109u64, &sponsor, &token_addr, &10_000i128, &200u64, &None);

    let max = 100 + crate::MAX_DEADLINE_EXTENSION;

    // Even the original sponsor cannot push past now + MAX_DEADLINE_EXTENSION.
    let err = client.try_extend_deadline(&109u64, &sponsor, &(max + 1));
    assert_eq!(err, Err(Ok(Error::InvalidDeadline)));
    let err = client.try_extend_deadline(&109u64, &sponsor, &u64::MAX);
    assert_eq!(err, Err(Ok(Error::InvalidDeadline)));

    // Exactly at the cap is allowed.
    client.extend_deadline(&109u64, &sponsor, &max);
    assert_eq!(client.get_escrow(&109u64).deadline, max);
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
fn test_pause_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, admin, _treasury, client) = setup(&env);

    // 1. No auths at all: require_admin auth must fail.
    env.set_auths(&[]);
    let result = client.try_pause();
    assert!(result.is_err());
    assert!(!client.is_paused_view());

    // 2. A non-admin signing the call is rejected too.
    let attacker = Address::generate(&env);
    let result = client
        .mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &attacker,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &client.address,
                fn_name: "pause",
                args: soroban_sdk::IntoVal::into_val(&(), &env),
                sub_invokes: &[],
            },
        }])
        .try_pause();
    assert!(result.is_err());
    assert!(!client.is_paused_view());

    // 3. Admin signing succeeds.
    let result = client
        .mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &admin,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &client.address,
                fn_name: "pause",
                args: soroban_sdk::IntoVal::into_val(&(), &env),
                sub_invokes: &[],
            },
        }])
        .try_pause();
    assert!(result.is_ok());
    assert!(client.is_paused_view());
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
fn test_pause_accepts_targeted_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, admin, _treasury, client) = setup(&env);

    // Only the admin's authorization is mocked for this pause call.
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "pause",
            args: soroban_sdk::vec![&env],
            sub_invokes: &[],
        },
    }]);

    client.pause();

    assert!(client.is_paused_view());
}

#[test]
fn test_pause_rejects_non_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _admin, _treasury, client) = setup(&env);
    let attacker = Address::generate(&env);

    // Mock a non-admin address as the signer: the admin's `require_auth`
    // must still fail with a host authorization error.
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &attacker,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "pause",
            args: soroban_sdk::vec![&env],
            sub_invokes: &[],
        },
    }]);

    assert_host_auth_error(|| client.pause());
    assert!(!client.is_paused_view());
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

    client.initialize(&admin, &oracle, &treasury, &500u32, &None, &None);
    assert_eq!(client.get_oracle(), oracle);
    assert_eq!(client.get_version(), CONTRACT_VERSION);

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

    // The furthest deadline allowed (MAX_DEADLINE_EXTENSION, 365 days) —
    // plus GRACE_PERIOD, the naive proportional ledger count for this
    // exceeds what Soroban actually allows a single persistent entry to
    // survive to. The record must still get *something* (the maximum this
    // call can grant), not silently fall back to the flat 29-day bump.
    let max_deadline: u64 = env.ledger().timestamp() + crate::MAX_DEADLINE_EXTENSION;
    client.extend_deadline(&302u64, &sponsor, &max_deadline);

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
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        let blocked_key = soroban_sdk::Symbol::new(&env, "blocked");
        if env.storage().instance().has(&blocked_key) {
            let blocked: Address = env.storage().instance().get(&blocked_key).unwrap();
            if to == blocked {
                panic!("Frozen/unauthorized trustline recipient");
            }
        }

        // Update balances
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
        env.storage()
            .persistent()
            .set(&(to_key, to), &(to_bal + amount));
    }

    pub fn set_blocked(env: Env, blocked: Address) {
        let blocked_key = soroban_sdk::Symbol::new(&env, "blocked");
        env.storage().instance().set(&blocked_key, &blocked);
    }

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
    panic_client.mint(&sponsor, &100_000i128);

    // Fund the escrow
    client.fund(
        &700u64,
        &sponsor,
        &token_addr,
        &10_000i128,
        &1_000u64,
        &None,
    );

    // Release to a team split where one recipient is blocked
    let recipients = vec![
        &env,
        (dev1.clone(), 4_000u32),
        (dev2.clone(), 4_000u32),
        (blocked_dev.clone(), 2_000u32),
    ];

    // The release call must revert (fail) due to the blocked recipient
    let result = client.try_release(&700u64, &recipients);
    assert!(
        result.is_err(),
        "Expected release to revert when one recipient is blocked"
    );

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

    client.fund(
        &800u64,
        &sponsor,
        &token_addr,
        &10_000i128,
        &1_000u64,
        &None,
    );
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

    client.fund(
        &801u64,
        &sponsor,
        &token_addr,
        &10_000i128,
        &1_000u64,
        &None,
    );
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

    client.fund(
        &810u64,
        &sponsor,
        &token_addr,
        &10_000i128,
        &1_000u64,
        &None,
    );
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

    client.fund(
        &804u64,
        &sponsor,
        &token_addr,
        &10_000i128,
        &1_000u64,
        &None,
    );
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

    client.fund(
        &805u64,
        &sponsor,
        &token_addr,
        &10_000i128,
        &1_000u64,
        &None,
    );
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
    client.fund(
        &806u64,
        &sponsor,
        &token_addr,
        &10_000i128,
        &1_000u64,
        &None,
    );
    let maintainer = Address::generate(&env);
    client.release(&806u64, &vec![&env, (maintainer, 10_000u32)]);
    assert_eq!(client.get_escrow(&806u64).status, EscrowStatus::Paid);

    client.fund(
        &806u64,
        &sponsor,
        &token_addr,
        &10_000i128,
        &2_000u64,
        &None,
    );
    assert_eq!(client.get_escrow(&806u64).status, EscrowStatus::Funded);

    // Refunded -> fund (re-fund) -> Funded
    client.fund(
        &807u64,
        &sponsor,
        &token_addr,
        &10_000i128,
        &1_000u64,
        &None,
    );
    client.refund(&807u64);
    assert_eq!(client.get_escrow(&807u64).status, EscrowStatus::Refunded);

    client.fund(
        &807u64,
        &sponsor,
        &token_addr,
        &10_000i128,
        &2_000u64,
        &None,
    );
    assert_eq!(client.get_escrow(&807u64).status, EscrowStatus::Funded);
}

// ─── set_fee_bps (issue #242) ────────────────────────────────────────────────

#[test]
fn test_set_fee_bps_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _admin, _treasury, client) = setup(&env);

    // No auths at all: the admin's require_auth must fail.
    env.set_auths(&[]);
    assert!(client.try_set_fee_bps(&600u32).is_err());

    // A non-admin signing the call is rejected too.
    let attacker = Address::generate(&env);
    let result = client
        .mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &attacker,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &contract_id,
                fn_name: "set_fee_bps",
                args: soroban_sdk::IntoVal::into_val(&(600u32,), &env),
                sub_invokes: &[],
            },
        }])
        .try_set_fee_bps(&600u32);
    assert!(result.is_err());

    assert_eq!(client.get_fee_bps(), 500u32);
}

#[test]
fn test_set_fee_bps_accepts_change_within_step_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env); // starts at 500 bps

    // Exactly +500 (the maximum step) is allowed.
    client.set_fee_bps(&1_000u32);
    assert_eq!(client.get_fee_bps(), 1_000u32);

    // Exactly -500 is allowed.
    client.set_fee_bps(&500u32);
    assert_eq!(client.get_fee_bps(), 500u32);

    // Small moves, including down to zero, are allowed.
    client.set_fee_bps(&250u32);
    client.set_fee_bps(&0u32);
    assert_eq!(client.get_fee_bps(), 0u32);
}

#[test]
fn test_set_fee_bps_rejects_change_exceeding_step_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env); // starts at 500 bps

    // +501 bps
    assert_eq!(
        client.try_set_fee_bps(&1_001u32),
        Err(Ok(Error::InvalidFee))
    );

    // -501 bps (after moving up to 1000 first)
    client.set_fee_bps(&1_000u32);
    assert_eq!(client.try_set_fee_bps(&499u32), Err(Ok(Error::InvalidFee)));

    // A large spike (the #20 scenario) is rejected outright.
    assert_eq!(
        client.try_set_fee_bps(&9_900u32),
        Err(Ok(Error::InvalidFee))
    );

    assert_eq!(client.get_fee_bps(), 1_000u32);
}

#[test]
fn test_set_fee_bps_rejects_value_above_bps_denominator() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env); // starts at 500 bps

    // Climb to exactly 10000 (100%) in valid +500 steps.
    let mut fee = 500u32;
    while fee < 10_000 {
        fee += 500;
        client.set_fee_bps(&fee);
    }
    assert_eq!(client.get_fee_bps(), 10_000u32);

    // One more step, even a tiny one, would exceed BPS_DENOMINATOR.
    assert_eq!(
        client.try_set_fee_bps(&10_001u32),
        Err(Ok(Error::InvalidFee))
    );
    assert_eq!(
        client.try_set_fee_bps(&10_500u32),
        Err(Ok(Error::InvalidFee))
    );
    assert_eq!(client.get_fee_bps(), 10_000u32);
}

/// Fee is read at release time, not snapshotted at fund time (#53): an escrow
/// funded before a fee change pays out using the updated fee.
#[test]
fn test_set_fee_bps_applies_to_already_funded_escrow_at_release() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, treasury, client) = setup(&env); // starts at 500 bps

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000i128);
    client.fund(
        &242u64,
        &sponsor,
        &token_addr,
        &10_000i128,
        &1_000u64,
        &None,
    );

    client.set_fee_bps(&1_000u32); // 5% -> 10%

    let contributor = Address::generate(&env);
    client.release(&242u64, &vec![&env, (contributor.clone(), 10_000u32)]);

    assert_eq!(token_client.balance(&treasury), 1_000i128);
    assert_eq!(token_client.balance(&contributor), 9_000i128);
}

#[test]
fn test_set_treasury_updates_treasury_address() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, treasury, client) = setup(&env);

    assert_eq!(client.get_treasury(), treasury);

    let new_treasury = Address::generate(&env);
    client.set_treasury(&new_treasury);
    assert_eq!(client.get_treasury(), new_treasury);
}

#[test]
fn test_set_treasury_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, admin, _treasury, client) = setup(&env);

    let new_treasury = Address::generate(&env);
    // Setting treasury without admin authorization will fail in unmocked environment
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_treasury",
            args: soroban_sdk::vec![&env, new_treasury.to_val()],
            sub_invokes: &[],
        },
    }]);

    client.set_treasury(&new_treasury);
    assert_eq!(client.get_treasury(), new_treasury);
}

#[test]
fn test_extend_deadline_returns_contribution_not_found_when_archived() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    env.ledger().set_timestamp(100);
    client.fund(
        &701u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &200u64,
        &None,
    );

    // Remove contribution sub-record to simulate archived persistent storage
    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .remove(&crate::types::DataKey::Contribution(701u64, 0));
    });

    let err = client.try_extend_deadline(&701u64, &sponsor, &500u64);
    assert_eq!(err, Err(Ok(Error::ContributionNotFound)));
}

#[test]
fn test_release_with_duplicate_recipient_addresses() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    let alice = Address::generate(&env);

    client.fund(
        &200u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );

    // Same address twice with 5000 bps each — must produce two separate
    // transfers summing to the full 10000-bps entitlement.
    let recipients = vec![&env, (alice.clone(), 5_000u32), (alice.clone(), 5_000u32)];
    client.release(&200u64, &recipients);

    let distributable = 950_0000000i128; // after 5% fee
    assert_eq!(token_client.balance(&alice), distributable);
    assert_eq!(token_client.balance(&treasury), 50_0000000i128);
}

#[test]
fn test_release_with_treasury_as_recipient() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    let alice = Address::generate(&env);

    client.fund(
        &201u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );

    // Treasury as one of the recipients — fee transfer and recipient transfer
    // are separate calls; treasury must receive fee + share without double-counting.
    let recipients = vec![
        &env,
        (alice.clone(), 5_000u32),
        (treasury.clone(), 5_000u32),
    ];
    client.release(&201u64, &recipients);

    let distributable = 950_0000000i128; // after 5% fee
    let alice_expected = distributable * 5000 / 10000;
    let treasury_expected = 50_0000000i128 + (distributable - alice_expected);
    assert_eq!(token_client.balance(&alice), alice_expected);
    assert_eq!(token_client.balance(&treasury), treasury_expected);
}

#[test]
fn test_release_rejects_self_payout() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &10_000_000_000i128);

    client.fund(
        &202u64,
        &sponsor,
        &token_addr,
        &10_000_000_000i128,
        &1_000u64,
        &None,
    );

    // Recipient equal to the contract's own address must be rejected.
    let recipients = vec![
        &env,
        (client.address.clone(), 5_000u32),
        (Address::generate(&env), 5_000u32),
    ];
    let err = client.try_release(&202u64, &recipients);
    assert_eq!(err, Err(Ok(Error::SelfPayout)));

    // Escrow must remain Funded (not Paid) so the funds are not stranded.
    let escrow = client.get_escrow(&202u64);
    assert_eq!(escrow.status, crate::types::EscrowStatus::Funded);
}

struct EscrowSequenceRng {
    state: u64,
}

impl EscrowSequenceRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 32) as u32
    }

    fn next_range(&mut self, upper_bound: u32) -> u32 {
        self.next_u32() % upper_bound
    }
}

fn assert_funded_escrow_solvency(
    client: &EscrowContractClient<'_>,
    token_client: &token::Client<'_>,
    contract_id: &Address,
    first_issue_id: u64,
    issue_count: u64,
) {
    let mut funded_amount = 0i128;
    for issue_id in first_issue_id..first_issue_id + issue_count {
        let escrow = match client.try_get_escrow(&issue_id) {
            Ok(Ok(escrow)) => escrow,
            Err(Ok(Error::EscrowNotFound)) => continue,
            result => panic!("unexpected get_escrow result for {issue_id}: {result:?}"),
        };
        if escrow.status == EscrowStatus::Funded {
            funded_amount = funded_amount
                .checked_add(escrow.amount)
                .expect("funded escrow sum should not overflow");
        }
    }

    assert!(
        token_client.balance(contract_id) >= funded_amount,
        "contract token balance must cover the sum of all funded escrows"
    );
}

#[test]
fn test_escrow_solvency_invariant_fuzzing() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsors = [
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    for sponsor in &sponsors {
        asset_client.mint(sponsor, &10_000_000i128);
    }
    let recipient = Address::generate(&env);
    let mut rng = EscrowSequenceRng::new(0x5eed);
    let first_issue_id = 900u64;
    let issue_count = 8u64;
    let mut exercised_actions = [false; 4];

    for _ in 0..500 {
        let issue_id = first_issue_id + rng.next_range(issue_count as u32) as u64;
        let sponsor = &sponsors[rng.next_range(sponsors.len() as u32) as usize];
        match rng.next_range(4) {
            0 => {
                let amount = rng.next_range(5_000) as i128 + 1;
                let deadline = env.ledger().timestamp() + 1_000;
                if matches!(
                    client.try_fund(&issue_id, sponsor, &token_addr, &amount, &deadline, &None),
                    Ok(Ok(()))
                ) {
                    exercised_actions[0] = true;
                }
            }
            1 => {
                let amount = rng.next_range(1_000) as i128 + 1;
                if matches!(
                    client.try_contribute(&issue_id, sponsor, &amount),
                    Ok(Ok(()))
                ) {
                    exercised_actions[1] = true;
                }
            }
            2 => {
                let recipients = vec![&env, (recipient.clone(), 10_000u32)];
                if matches!(client.try_release(&issue_id, &recipients), Ok(Ok(()))) {
                    exercised_actions[2] = true;
                }
            }
            3 => {
                if matches!(client.try_refund(&issue_id), Ok(Ok(()))) {
                    exercised_actions[3] = true;
                }
            }
            _ => unreachable!(),
        }

        assert_funded_escrow_solvency(
            &client,
            &token_client,
            &contract_id,
            first_issue_id,
            issue_count,
        );
    }

    assert!(
        exercised_actions.into_iter().all(|exercised| exercised),
        "the randomized sequence must successfully exercise fund, contribute, release, and refund"
    );
}

// ── upgrade tests (#461) ──────────────────────────────────────────────────────

#[test]
fn test_upgrade_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();
    let (_contract_id, _admin, _treasury, client) = setup(&env);

    let fake_hash = BytesN::from_array(&env, &[0u8; 32]);
    // With no authorizations recorded, upgrade must reject with NotAuthorized.
    env.set_auths(&[]);
    let err = client.try_upgrade(&fake_hash);
    assert!(err.is_err());
}

#[test]
fn test_upgrade_requires_explicit_idempotent_migration() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, _admin, _treasury, client) = setup(&env);

    assert_eq!(client.get_version(), CONTRACT_VERSION);

    // The old implementation's upgrade call must leave its stored version
    // intact; only the newly installed implementation can advance it.
    env.as_contract(&client.address, || {
        env.storage().instance().set(&DataKey::Version, &1u32);
    });
    let wasm_hash = existing_wasm_hash(&env);
    client.upgrade(&wasm_hash);
    assert_eq!(client.get_version(), 1u32);

    client.migrate(&1u32);
    assert_eq!(client.get_version(), CONTRACT_VERSION);
    client.migrate(&1u32);
    assert_eq!(client.get_version(), CONTRACT_VERSION);
}

#[test]
fn test_migrate_rejects_unauthorized_and_mismatched_versions() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, _admin, _treasury, client) = setup(&env);

    env.set_auths(&[]);
    assert!(client.try_migrate(&1u32).is_err());

    // Simulate an old implementation at version 1, then reject a migration
    // from a version that does not match the stored one.
    env.as_contract(&client.address, || {
        env.storage().instance().set(&DataKey::Version, &1u32);
    });
    env.mock_all_auths();
    let err = client.try_migrate(&0u32);
    assert_eq!(err, Err(Ok(Error::InvalidVersion)));
}

#[test]
fn test_upgrade_preserves_existing_escrows() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, _admin, _treasury, client) = setup(&env);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _token_client) = create_token(&env, &token_admin);
    let sponsor = Address::generate(&env);
    asset_client.mint(&sponsor, &5_000_000_000i128);

    client.fund(
        &300u64,
        &sponsor,
        &token_addr,
        &5_000_000_000i128,
        &2_000u64,
        &None,
    );

    let wasm_hash = existing_wasm_hash(&env);
    client.upgrade(&wasm_hash);

    // Escrow data must survive the upgrade intact.
    let escrow = client.get_escrow(&300u64);
    assert_eq!(escrow.status, crate::types::EscrowStatus::Funded);
    assert_eq!(escrow.deadline, 2_000u64);
}

#[test]
fn test_set_admin_updates_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, admin, _, client) = setup(&env);

    assert_eq!(client.get_admin(), admin);

    let new_admin = Address::generate(&env);
    client.set_admin(&new_admin);
    assert_eq!(client.get_admin(), new_admin);
}

#[test]
fn test_set_admin_requires_current_and_new_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _admin, _, client) = setup(&env);

    let new_admin = Address::generate(&env);
    client.set_admin(&new_admin);
    // After rotation, the new admin should be in storage.
    assert_eq!(client.get_admin(), new_admin);

    // Rotating again from new_admin to a third address must also succeed.
    let third_admin = Address::generate(&env);
    client.set_admin(&third_admin);
    assert_eq!(client.get_admin(), third_admin);
}
