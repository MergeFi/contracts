#![cfg(test)]

extern crate std;

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token, Address, BytesN, Env, Vec,
};

const SOROBAN_MAX_CPU_INSTRUCTIONS: u64 = 100_000_000;
const SOROBAN_MAX_MEMORY_BYTES: u64 = 41_943_040;
const REGRESSION_TOLERANCE_BPS: u64 = 15_000; // 150% of baseline

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

fn native_wasm_hash(env: &Env) -> BytesN<32> {
    env.crypto().sha256(&soroban_sdk::Bytes::new(env)).into()
}

fn measure(env: &Env, f: impl FnOnce()) -> (u64, u64) {
    env.cost_estimate().budget().reset_default();
    f();
    let cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let mem = env.cost_estimate().budget().memory_bytes_cost();
    (cpu, mem)
}

fn check_budget(label: &str, cpu: u64, mem: u64, baseline_cpu: u64, baseline_mem: u64) {
    let max_cpu = (baseline_cpu * REGRESSION_TOLERANCE_BPS) / 10_000;
    let max_mem = (baseline_mem * REGRESSION_TOLERANCE_BPS) / 10_000;
    std::println!(
        "| {:<42} | {:>12} | {:>12} | {:>12} | {:>12} |",
        label,
        cpu,
        mem,
        max_cpu,
        max_mem
    );
    assert!(cpu > 0, "{label}: measured CPU instructions must be > 0");
    assert!(mem > 0, "{label}: measured memory bytes must be > 0");
    assert!(
        cpu <= max_cpu && cpu <= SOROBAN_MAX_CPU_INSTRUCTIONS,
        "{label}: CPU instruction regression: measured {cpu} > ceiling {max_cpu} (baseline {baseline_cpu})"
    );
    assert!(
        mem <= max_mem && mem <= SOROBAN_MAX_MEMORY_BYTES,
        "{label}: memory byte regression: measured {mem} > ceiling {max_mem} (baseline {baseline_mem})"
    );
}

fn build_recipients(env: &Env, count: u32) -> Vec<(Address, u32)> {
    let mut recipients = Vec::new(env);
    let base_share = 10_000u32 / count;
    let remainder = 10_000u32 % count;
    for i in 0..count {
        let addr = Address::generate(env);
        let share = if i == 0 {
            base_share + remainder
        } else {
            base_share
        };
        recipients.push_back((addr, share));
    }
    recipients
}

#[test]
fn bench_escrow_entrypoints_and_scaling() {
    std::println!("\n=== mergefi-escrow Resource Budget Benchmark ===");
    std::println!(
        "| {:<42} | {:>12} | {:>12} | {:>12} | {:>12} |",
        "Entrypoint / Scenario",
        "CPU Instrs",
        "Mem Bytes",
        "CPU Ceiling",
        "Mem Ceiling"
    );
    std::println!(
        "|{:-<44}|{:-<14}|{:-<14}|{:-<14}|{:-<14}|",
        "",
        "",
        "",
        "",
        ""
    );

    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().disable_resource_limits();

    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let treasury = Address::generate(&env);
    let recovery = Address::generate(&env);
    let contract_id = env.register(EscrowContract, ());
    let client = EscrowContractClient::new(&env, &contract_id);

    let (cpu, mem) = measure(&env, || {
        client.initialize(
            &admin,
            &oracle,
            &treasury,
            &500u32,
            &None,
            &Some(recovery.clone()),
        );
    });
    check_budget("initialize", cpu, mem, 95_000, 12_000);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, _) = create_token(&env, &token_admin);
    let sponsor_a = Address::generate(&env);
    let sponsor_b = Address::generate(&env);
    asset_client.mint(&sponsor_a, &100_000_000i128);
    asset_client.mint(&sponsor_b, &100_000_000i128);

    let (cpu, mem) = measure(&env, || {
        client.fund(
            &1u64,
            &sponsor_a,
            &token_addr,
            &1_000_000i128,
            &10_000u64,
            &None,
        );
    });
    check_budget("fund", cpu, mem, 450_000, 70_000);

    let (cpu, mem) = measure(&env, || {
        client.contribute(&1u64, &sponsor_b, &500_000i128);
    });
    check_budget("contribute", cpu, mem, 500_000, 75_000);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_escrow(&1u64);
    });
    check_budget("get_escrow", cpu, mem, 52_000, 9_000);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_contribution(&1u64, &0u32);
    });
    check_budget("get_contribution", cpu, mem, 48_000, 8_000);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_contributions(&1u64);
    });
    check_budget("get_contributions (2 sponsors)", cpu, mem, 82_000, 11_000);

    let (cpu, mem) = measure(&env, || {
        client.extend_deadline(&1u64, &sponsor_a, &20_000u64);
    });
    check_budget("extend_deadline", cpu, mem, 170_000, 26_000);

    let (cpu, mem) = measure(&env, || {
        client.keep_alive(&1u64);
    });
    check_budget("keep_alive (2 sponsors)", cpu, mem, 70_000, 10_000);

    // Admin & view entrypoints
    let (cpu, mem) = measure(&env, || {
        let _ = client.get_admin();
    });
    check_budget("get_admin", cpu, mem, 50_000, 8_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_oracle();
    });
    check_budget("get_oracle", cpu, mem, 50_000, 8_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_treasury();
    });
    check_budget("get_treasury", cpu, mem, 50_000, 8_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_fee_bps();
    });
    check_budget("get_fee_bps", cpu, mem, 50_000, 8_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_version();
    });
    check_budget("get_version", cpu, mem, 50_000, 8_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_max_sponsors();
    });
    check_budget("get_max_sponsors", cpu, mem, 50_000, 8_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_grace_period();
    });
    check_budget("get_grace_period", cpu, mem, 32_000, 6_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_bps_denominator();
    });
    check_budget("get_bps_denominator", cpu, mem, 32_000, 6_500);

    let (cpu, mem) = measure(&env, || {
        client.set_fee_bps(&600u32);
    });
    check_budget("set_fee_bps", cpu, mem, 125_000, 22_000);
    client.set_fee_bps(&500u32);

    let new_admin = Address::generate(&env);
    let (cpu, mem) = measure(&env, || {
        client.set_admin(&new_admin);
    });
    check_budget("set_admin", cpu, mem, 140_000, 29_000);

    let (cpu, mem) = measure(&env, || {
        client.recover_admin(&admin);
    });
    check_budget("recover_admin", cpu, mem, 140_000, 30_000);

    let new_oracle = Address::generate(&env);
    let (cpu, mem) = measure(&env, || {
        client.set_oracle(&new_oracle);
    });
    check_budget("set_oracle", cpu, mem, 145_000, 32_000);

    let new_treasury = Address::generate(&env);
    let (cpu, mem) = measure(&env, || {
        client.set_treasury(&new_treasury);
    });
    check_budget("set_treasury", cpu, mem, 120_000, 25_000);

    let (cpu, mem) = measure(&env, || {
        client.pause();
    });
    check_budget("pause", cpu, mem, 118_000, 25_000);

    let (cpu, mem) = measure(&env, || {
        let _ = client.is_paused_view();
    });
    check_budget("is_paused_view", cpu, mem, 47_000, 9_500);

    let (cpu, mem) = measure(&env, || {
        client.unpause();
    });
    check_budget("unpause", cpu, mem, 120_000, 25_000);

    let (cpu, mem) = measure(&env, || {
        client.migrate(&CONTRACT_VERSION);
    });
    check_budget("migrate (idempotent)", cpu, mem, 90_000, 18_000);

    // Parameterized scaling: release by recipient count (1, 5, 10, 20) in isolated Env per N
    for &(n, base_cpu, base_mem) in &[
        (1u32, 550_000u64, 95_000u64),
        (5u32, 1_450_000u64, 240_000u64),
        (10u32, 2_750_000u64, 460_000u64),
        (20u32, 5_500_000u64, 920_000u64),
    ] {
        let s_env = Env::default();
        s_env.mock_all_auths();
        s_env.cost_estimate().disable_resource_limits();
        let s_admin = Address::generate(&s_env);
        let s_oracle = Address::generate(&s_env);
        let s_treasury = Address::generate(&s_env);
        let s_cid = s_env.register(EscrowContract, ());
        let s_client = EscrowContractClient::new(&s_env, &s_cid);
        s_client.initialize(&s_admin, &s_oracle, &s_treasury, &500u32, &None, &None);
        let s_tok_admin = Address::generate(&s_env);
        let (s_tok, s_asset, _) = create_token(&s_env, &s_tok_admin);
        let s_sponsor = Address::generate(&s_env);
        s_asset.mint(&s_sponsor, &10_000_000i128);
        s_client.fund(&1u64, &s_sponsor, &s_tok, &1_000_000i128, &50_000u64, &None);
        let recipients = build_recipients(&s_env, n);
        let label = std::format!("release (recipients={n})");
        let (cpu, mem) = measure(&s_env, || {
            s_client.release(&1u64, &recipients);
        });
        check_budget(&label, cpu, mem, base_cpu, base_mem);
    }

    // Parameterized scaling: refund by contributor count (1, 5, 10, 20) in isolated Env per N
    for &(n, base_cpu, base_mem) in &[
        (1u32, 420_000u64, 70_000u64),
        (5u32, 1_350_000u64, 225_000u64),
        (10u32, 2_600_000u64, 435_000u64),
        (20u32, 5_200_000u64, 880_000u64),
    ] {
        let s_env = Env::default();
        s_env.mock_all_auths();
        s_env.cost_estimate().disable_resource_limits();
        let s_admin = Address::generate(&s_env);
        let s_oracle = Address::generate(&s_env);
        let s_treasury = Address::generate(&s_env);
        let s_cid = s_env.register(EscrowContract, ());
        let s_client = EscrowContractClient::new(&s_env, &s_cid);
        s_client.initialize(&s_admin, &s_oracle, &s_treasury, &500u32, &None, &None);
        let s_tok_admin = Address::generate(&s_env);
        let (s_tok, s_asset, _) = create_token(&s_env, &s_tok_admin);
        let first_sponsor = Address::generate(&s_env);
        s_asset.mint(&first_sponsor, &100_000i128);
        s_client.fund(
            &1u64,
            &first_sponsor,
            &s_tok,
            &100_000i128,
            &50_000u64,
            &None,
        );
        for _ in 1..n {
            let s = Address::generate(&s_env);
            s_asset.mint(&s, &100_000i128);
            s_client.contribute(&1u64, &s, &100_000i128);
        }
        let label = std::format!("refund (contributors={n})");
        let (cpu, mem) = measure(&s_env, || {
            s_client.refund(&1u64);
        });
        check_budget(&label, cpu, mem, base_cpu, base_mem);
    }

    // Permissionless refund after deadline + grace period
    env.cost_estimate().disable_resource_limits();
    client.fund(
        &300u64,
        &sponsor_a,
        &token_addr,
        &100_000i128,
        &1_000u64,
        &None,
    );
    env.ledger().set_timestamp(1_000 + GRACE_PERIOD + 1);
    let (cpu, mem) = measure(&env, || {
        client.refund(&300u64);
    });
    check_budget(
        "refund (permissionless after deadline)",
        cpu,
        mem,
        450_000,
        80_000,
    );

    // Upgrade
    let wasm_hash = native_wasm_hash(&env);
    let (cpu, mem) = measure(&env, || {
        client.upgrade(&wasm_hash);
    });
    check_budget("upgrade", cpu, mem, 160_000, 35_000);
}
