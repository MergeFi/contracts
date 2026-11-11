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
fn bench_maintenance_pool_entrypoints_and_scaling() {
    std::println!("\n=== mergefi-maintenance-pool Resource Budget Benchmark ===");
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
    let contract_id = env.register(MaintenancePoolContract, ());
    let client = MaintenancePoolContractClient::new(&env, &contract_id);

    let (cpu, mem) = measure(&env, || {
        client.initialize(
            &admin,
            &oracle,
            &treasury,
            &1_000u32,
            &Some(recovery.clone()),
        );
    });
    check_budget("initialize", cpu, mem, 95_000, 12_000);

    let token_admin = Address::generate(&env);
    let (token_addr, asset_client, token_client) = create_token(&env, &token_admin);
    let sponsor_a = Address::generate(&env);
    asset_client.mint(&sponsor_a, &1_000_000_000i128);

    let (cpu, mem) = measure(&env, || {
        client.deposit(&1u64, &sponsor_a, &token_addr, &1_000_000i128);
    });
    check_budget("deposit (initial pool creation)", cpu, mem, 480_000, 75_000);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_pool(&1u64);
    });
    check_budget("get_pool", cpu, mem, 55_000, 9_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_deposit(&1u64, &0u32);
    });
    check_budget("get_deposit", cpu, mem, 55_000, 9_500);

    token_client.transfer(&sponsor_a, &contract_id, &500i128);
    let sweep_recipient = Address::generate(&env);
    let (cpu, mem) = measure(&env, || {
        let _ = client.sweep(&1u64, &token_addr, &sweep_recipient);
    });
    check_budget("sweep", cpu, mem, 480_000, 80_000);

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
        let _ = client.get_bps_denominator();
    });
    check_budget("get_bps_denominator", cpu, mem, 35_000, 7_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_max_deposits();
    });
    check_budget("get_max_deposits", cpu, mem, 35_000, 7_500);

    let (cpu, mem) = measure(&env, || {
        let _ = client.get_inactivity_window();
    });
    check_budget("get_inactivity_window", cpu, mem, 35_000, 7_500);

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

    // Parameterized scaling: deposit, keep_alive, withdraw (1 recipient), reclaim_deposit by deposit_count (1, 10, 25, 50)
    for &(dep_count, dep_cpu, dep_mem, ka_cpu, ka_mem, wd_cpu, wd_mem, rec_cpu, rec_mem) in &[
        (
            1u32, 430_000u64, 66_000u64, 60_000u64, 8_500u64, 500_000u64, 70_000u64, 340_000u64,
            48_000u64,
        ),
        (
            10u32, 830_000u64, 125_000u64, 165_000u64, 18_000u64, 730_000u64, 120_000u64,
            590_000u64, 86_000u64,
        ),
        (
            25u32,
            1_550_000u64,
            230_000u64,
            410_000u64,
            38_000u64,
            1_150_000u64,
            210_000u64,
            1_100_000u64,
            155_000u64,
        ),
        (
            50u32,
            2_950_000u64,
            410_000u64,
            1_000_000u64,
            85_000u64,
            1_960_000u64,
            375_000u64,
            2_120_000u64,
            285_000u64,
        ),
    ] {
        let s_env = Env::default();
        s_env.mock_all_auths();
        s_env.cost_estimate().disable_resource_limits();
        s_env.ledger().set_timestamp(100);
        let s_admin = Address::generate(&s_env);
        let s_oracle = Address::generate(&s_env);
        let s_treasury = Address::generate(&s_env);
        let s_cid = s_env.register(MaintenancePoolContract, ());
        let s_client = MaintenancePoolContractClient::new(&s_env, &s_cid);
        s_client.initialize(&s_admin, &s_oracle, &s_treasury, &1_000u32, &None);
        let s_tok_admin = Address::generate(&s_env);
        let (s_tok, s_asset, _) = create_token(&s_env, &s_tok_admin);
        for _ in 1..dep_count {
            let sp = Address::generate(&s_env);
            s_asset.mint(&sp, &100_000i128);
            s_client.deposit(&1u64, &sp, &s_tok, &100_000i128);
        }
        let nth_sponsor = Address::generate(&s_env);
        s_asset.mint(&nth_sponsor, &100_000i128);
        let dep_label = std::format!("deposit (deposit_count={dep_count})");
        let (cpu, mem) = measure(&s_env, || {
            s_client.deposit(&1u64, &nth_sponsor, &s_tok, &100_000i128);
        });
        check_budget(&dep_label, cpu, mem, dep_cpu, dep_mem);

        let ka_label = std::format!("keep_alive (deposit_count={dep_count})");
        let (cpu, mem) = measure(&s_env, || {
            s_client.keep_alive(&1u64);
        });
        check_budget(&ka_label, cpu, mem, ka_cpu, ka_mem);

        let recipients = build_recipients(&s_env, 1);
        let wd_label = std::format!("withdraw (deposit_count={dep_count}, recipients=1)");
        let (cpu, mem) = measure(&s_env, || {
            s_client.withdraw(&1u64, &recipients, &10_000i128);
        });
        check_budget(&wd_label, cpu, mem, wd_cpu, wd_mem);

        s_env.ledger().set_timestamp(100 + INACTIVITY_WINDOW + 1);
        let rec_label = std::format!("reclaim_deposit (deposit_count={dep_count})");
        let (cpu, mem) = measure(&s_env, || {
            s_client.reclaim_deposit(&1u64, &(dep_count - 1), &nth_sponsor);
        });
        check_budget(&rec_label, cpu, mem, rec_cpu, rec_mem);
    }

    // Parameterized scaling: withdraw by recipient count (1, 5, 10, 20)
    for &(n, base_cpu, base_mem) in &[
        (1u32, 650_000u64, 110_000u64),
        (5u32, 1_550_000u64, 260_000u64),
        (10u32, 2_850_000u64, 480_000u64),
        (20u32, 5_600_000u64, 950_000u64),
    ] {
        let s_env = Env::default();
        s_env.mock_all_auths();
        s_env.cost_estimate().disable_resource_limits();
        s_env.ledger().set_timestamp(200);
        let s_admin = Address::generate(&s_env);
        let s_oracle = Address::generate(&s_env);
        let s_treasury = Address::generate(&s_env);
        let s_cid = s_env.register(MaintenancePoolContract, ());
        let s_client = MaintenancePoolContractClient::new(&s_env, &s_cid);
        s_client.initialize(&s_admin, &s_oracle, &s_treasury, &1_000u32, &None);
        let s_tok_admin = Address::generate(&s_env);
        let (s_tok, s_asset, _) = create_token(&s_env, &s_tok_admin);
        let s_sponsor = Address::generate(&s_env);
        s_asset.mint(&s_sponsor, &10_000_000i128);
        s_client.deposit(&1u64, &s_sponsor, &s_tok, &10_000_000i128);
        let recipients = build_recipients(&s_env, n);
        let label = std::format!("withdraw (recipients={n})");
        let (cpu, mem) = measure(&s_env, || {
            s_client.withdraw(&1u64, &recipients, &1_000_000i128);
        });
        check_budget(&label, cpu, mem, base_cpu, base_mem);
    }

    // Upgrade
    let wasm_hash = native_wasm_hash(&env);
    let (cpu, mem) = measure(&env, || {
        client.upgrade(&wasm_hash);
    });
    check_budget("upgrade", cpu, mem, 160_000, 35_000);
}
