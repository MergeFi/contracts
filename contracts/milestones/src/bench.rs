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
        "| {:<46} | {:>12} | {:>12} | {:>12} | {:>12} |",
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
fn bench_milestones_entrypoints_and_scaling() {
    std::println!("\n=== mergefi-milestones Resource Budget Benchmark ===");
    std::println!(
        "| {:<46} | {:>12} | {:>12} | {:>12} | {:>12} |",
        "Entrypoint / Scenario",
        "CPU Instrs",
        "Mem Bytes",
        "CPU Ceiling",
        "Mem Ceiling"
    );
    std::println!(
        "|{:-<48}|{:-<14}|{:-<14}|{:-<14}|{:-<14}|",
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
    let contract_id = env.register(MilestonesContract, ());
    let client = MilestonesContractClient::new(&env, &contract_id);

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
    asset_client.mint(&sponsor_a, &1_000_000_000i128);
    asset_client.mint(&sponsor_b, &1_000_000_000i128);

    let (cpu, mem) = measure(&env, || {
        client.create_milestone(&1u64, &sponsor_a, &token_addr, &10_000_000i128, &50_000u64);
    });
    check_budget("create_milestone", cpu, mem, 480_000, 75_000);

    let (cpu, mem) = measure(&env, || {
        client.contribute(&1u64, &sponsor_b, &5_000_000i128);
    });
    check_budget("contribute", cpu, mem, 520_000, 80_000);

    let (cpu, mem) = measure(&env, || {
        client.allocate(&1u64, &10u64, &1_000_000i128);
    });
    check_budget("allocate (initial)", cpu, mem, 250_000, 45_000);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_milestone(&1u64);
    });
    check_budget("get_milestone", cpu, mem, 60_000, 10_000);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_issue_status(&1u64, &10u64);
    });
    check_budget("get_issue_status", cpu, mem, 55_000, 9_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_contribution(&1u64, &0u32);
    });
    check_budget("get_contribution", cpu, mem, 55_000, 9_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_contributions(&1u64);
    });
    check_budget("get_contributions (2 sponsors)", cpu, mem, 95_000, 14_000);

    let (cpu, mem) = measure(&env, || {
        client.deallocate(&1u64, &10u64);
    });
    check_budget("deallocate", cpu, mem, 250_000, 45_000);

    client.allocate(&1u64, &10u64, &1_000_000i128);
    let (cpu, mem) = measure(&env, || {
        client.keep_alive(&1u64);
    });
    check_budget(
        "keep_alive (2 sponsors, 1 issue)",
        cpu,
        mem,
        100_000,
        15_000,
    );

    // Admin & view entrypoints
    let (cpu, mem) = measure(&env, || {
        let _ = client.get_admin();
    });
    check_budget("get_admin", cpu, mem, 55_000, 9_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_oracle();
    });
    check_budget("get_oracle", cpu, mem, 55_000, 9_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_treasury();
    });
    check_budget("get_treasury", cpu, mem, 55_000, 9_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_fee_bps();
    });
    check_budget("get_fee_bps", cpu, mem, 55_000, 9_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_version();
    });
    check_budget("get_version", cpu, mem, 55_000, 9_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_max_sponsors();
    });
    check_budget("get_max_sponsors", cpu, mem, 55_000, 9_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_grace_period();
    });
    check_budget("get_grace_period", cpu, mem, 35_000, 7_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_bps_denominator();
    });
    check_budget("get_bps_denominator", cpu, mem, 35_000, 7_500);

    let (cpu, mem) = measure(&env, || {
        client.set_fee_bps(&600u32);
    });
    check_budget("set_fee_bps", cpu, mem, 130_000, 25_000);
    client.set_fee_bps(&500u32);

    let new_admin = Address::generate(&env);
    let (cpu, mem) = measure(&env, || {
        client.set_admin(&new_admin);
    });
    check_budget("set_admin", cpu, mem, 145_000, 32_000);

    let (cpu, mem) = measure(&env, || {
        client.recover_admin(&admin);
    });
    check_budget("recover_admin", cpu, mem, 145_000, 32_000);

    let new_oracle = Address::generate(&env);
    let (cpu, mem) = measure(&env, || {
        client.set_oracle(&new_oracle);
    });
    check_budget("set_oracle", cpu, mem, 150_000, 35_000);

    let new_treasury = Address::generate(&env);
    let (cpu, mem) = measure(&env, || {
        client.set_treasury(&new_treasury);
    });
    check_budget("set_treasury", cpu, mem, 125_000, 28_000);

    let (cpu, mem) = measure(&env, || {
        client.pause();
    });
    check_budget("pause", cpu, mem, 125_000, 28_000);

    let (cpu, mem) = measure(&env, || {
        let _ = client.is_paused_view();
    });
    check_budget("is_paused_view", cpu, mem, 50_000, 10_000);

    let (cpu, mem) = measure(&env, || {
        client.unpause();
    });
    check_budget("unpause", cpu, mem, 125_000, 28_000);

    let (cpu, mem) = measure(&env, || {
        client.migrate(&CONTRACT_VERSION);
    });
    check_budget("migrate (idempotent)", cpu, mem, 95_000, 20_000);

    // Parameterized scaling: allocate & release_issue by prior allocation count (1, 10, 25, 50)
    for &(alloc_count, alloc_cpu, alloc_mem, rel_cpu, rel_mem) in &[
        (1u64, 250_000u64, 45_000u64, 650_000u64, 110_000u64),
        (10u64, 350_000u64, 65_000u64, 750_000u64, 130_000u64),
        (25u64, 550_000u64, 105_000u64, 950_000u64, 165_000u64),
        (50u64, 950_000u64, 180_000u64, 1_350_000u64, 240_000u64),
    ] {
        let s_env = Env::default();
        s_env.mock_all_auths();
        s_env.cost_estimate().disable_resource_limits();
        let s_admin = Address::generate(&s_env);
        let s_oracle = Address::generate(&s_env);
        let s_treasury = Address::generate(&s_env);
        let s_cid = s_env.register(MilestonesContract, ());
        let s_client = MilestonesContractClient::new(&s_env, &s_cid);
        s_client.initialize(&s_admin, &s_oracle, &s_treasury, &500u32, &None, &None);
        let s_tok_admin = Address::generate(&s_env);
        let (s_tok, s_asset, _) = create_token(&s_env, &s_tok_admin);
        let s_sponsor = Address::generate(&s_env);
        s_asset.mint(&s_sponsor, &100_000_000i128);
        s_client.create_milestone(&1u64, &s_sponsor, &s_tok, &100_000_000i128, &50_000u64);
        for idx in 1..alloc_count {
            s_client.allocate(&1u64, &idx, &10_000i128);
        }
        let alloc_label = std::format!("allocate (allocations={alloc_count})");
        let (cpu, mem) = measure(&s_env, || {
            s_client.allocate(&1u64, &alloc_count, &10_000i128);
        });
        check_budget(&alloc_label, cpu, mem, alloc_cpu, alloc_mem);

        let recipients = build_recipients(&s_env, 1);
        let rel_label = std::format!("release_issue (allocations={alloc_count}, recipients=1)");
        let (cpu, mem) = measure(&s_env, || {
            s_client.release_issue(&1u64, &alloc_count, &recipients);
        });
        check_budget(&rel_label, cpu, mem, rel_cpu, rel_mem);
    }

    // Parameterized scaling: release_issue by recipient count (1, 5, 10, 20)
    for &(n, base_cpu, base_mem) in &[
        (1u32, 650_000u64, 110_000u64),
        (5u32, 1_550_000u64, 260_000u64),
        (10u32, 2_850_000u64, 480_000u64),
        (20u32, 5_600_000u64, 950_000u64),
    ] {
        let s_env = Env::default();
        s_env.mock_all_auths();
        s_env.cost_estimate().disable_resource_limits();
        let s_admin = Address::generate(&s_env);
        let s_oracle = Address::generate(&s_env);
        let s_treasury = Address::generate(&s_env);
        let s_cid = s_env.register(MilestonesContract, ());
        let s_client = MilestonesContractClient::new(&s_env, &s_cid);
        s_client.initialize(&s_admin, &s_oracle, &s_treasury, &500u32, &None, &None);
        let s_tok_admin = Address::generate(&s_env);
        let (s_tok, s_asset, _) = create_token(&s_env, &s_tok_admin);
        let s_sponsor = Address::generate(&s_env);
        s_asset.mint(&s_sponsor, &10_000_000i128);
        s_client.create_milestone(&1u64, &s_sponsor, &s_tok, &10_000_000i128, &50_000u64);
        s_client.allocate(&1u64, &1u64, &1_000_000i128);
        let recipients = build_recipients(&s_env, n);
        let label = std::format!("release_issue (recipients={n})");
        let (cpu, mem) = measure(&s_env, || {
            s_client.release_issue(&1u64, &1u64, &recipients);
        });
        check_budget(&label, cpu, mem, base_cpu, base_mem);
    }

    // Parameterized scaling: cancel_milestone by contributor count (1, 5, 10, 20)
    for &(n, base_cpu, base_mem) in &[
        (1u32, 450_000u64, 75_000u64),
        (5u32, 1_400_000u64, 235_000u64),
        (10u32, 2_650_000u64, 445_000u64),
        (20u32, 5_300_000u64, 900_000u64),
    ] {
        let s_env = Env::default();
        s_env.mock_all_auths();
        s_env.cost_estimate().disable_resource_limits();
        let s_admin = Address::generate(&s_env);
        let s_oracle = Address::generate(&s_env);
        let s_treasury = Address::generate(&s_env);
        let s_cid = s_env.register(MilestonesContract, ());
        let s_client = MilestonesContractClient::new(&s_env, &s_cid);
        s_client.initialize(&s_admin, &s_oracle, &s_treasury, &500u32, &None, &None);
        let s_tok_admin = Address::generate(&s_env);
        let (s_tok, s_asset, _) = create_token(&s_env, &s_tok_admin);
        let first_sponsor = Address::generate(&s_env);
        s_asset.mint(&first_sponsor, &100_000i128);
        s_client.create_milestone(&1u64, &first_sponsor, &s_tok, &100_000i128, &50_000u64);
        for _ in 1..n {
            let s = Address::generate(&s_env);
            s_asset.mint(&s, &100_000i128);
            s_client.contribute(&1u64, &s, &100_000i128);
        }
        let label = std::format!("cancel_milestone (contributors={n})");
        let (cpu, mem) = measure(&s_env, || {
            s_client.cancel_milestone(&1u64);
        });
        check_budget(&label, cpu, mem, base_cpu, base_mem);
    }

    // Permissionless cancel_milestone_after_deadline
    env.cost_estimate().disable_resource_limits();
    client.create_milestone(&400u64, &sponsor_a, &token_addr, &100_000i128, &1_000u64);
    env.ledger().set_timestamp(1_000 + GRACE_PERIOD + 1);
    let (cpu, mem) = measure(&env, || {
        client.cancel_milestone_after_deadline(&400u64);
    });
    check_budget("cancel_milestone_after_deadline", cpu, mem, 480_000, 85_000);

    // Upgrade
    let wasm_hash = native_wasm_hash(&env);
    let (cpu, mem) = measure(&env, || {
        client.upgrade(&wasm_hash);
    });
    check_budget("upgrade", cpu, mem, 160_000, 35_000);
}
