# Soroban Instruction & Resource-Budget Benchmarks

This document records the baseline Soroban CPU instruction (`cpu_instruction_cost`) and memory byte (`memory_bytes_cost`) measurements for every public entrypoint across `mergefi-escrow`, `mergefi-milestones`, and `mergefi-maintenance-pool`, including size-parameterized scaling curves for bounded and unbounded on-chain collections.

## Running the Benchmarks

Use the `bench` target in the root `Makefile`:

```sh
make bench
```

Or invoke `cargo test` directly on the workspace:

```sh
cargo test --workspace bench_ -- --nocapture --test-threads=1
```

## Methodology & Regression Detection

1. **Measurement API**: Each entrypoint invocation is wrapped in a helper that calls `env.cost_estimate().budget().reset_default()` immediately before the contract call and reads `env.cost_estimate().budget().cpu_instruction_cost()` and `env.cost_estimate().budget().memory_bytes_cost()` immediately after the call returns.
2. **Isolated Scaling Environments**: Size-parameterized scaling benchmarks (`recipients`, `contributors`, `allocations`, `deposit_count`) construct a fresh `Env::default()` per scaling point `N` so that host shadow state from prior scenarios does not skew the curve.
3. **Regression Tolerance Band**: Every benchmark test asserts:
   - `cpu > 0` and `mem > 0`
   - `cpu <= baseline_cpu * 1.50` and `mem <= baseline_mem * 1.50` (`REGRESSION_TOLERANCE_BPS = 15_000`)
   - `cpu <= 100_000_000` (Soroban mainnet per-transaction CPU instruction limit)
   - `mem <= 41_943_040` (Soroban mainnet 40 MiB per-transaction memory byte limit)

---

## 1. `mergefi-escrow` Baseline & Scaling Curves

### Entrypoint Baselines

| Entrypoint / Scenario | Measured CPU Instrs | Measured Mem Bytes | Regression CPU Ceiling | Regression Mem Ceiling |
|---|---:|---:|---:|---:|
| `initialize` | 94,360 | 11,353 | 142,500 | 18,000 |
| `fund` | 441,119 | 69,382 | 675,000 | 105,000 |
| `contribute` | 492,025 | 73,453 | 750,000 | 112,500 |
| `get_escrow` | 50,657 | 8,275 | 78,000 | 13,500 |
| `get_contribution` | 47,065 | 7,745 | 72,000 | 12,000 |
| `get_contributions (2 sponsors)` | 81,061 | 10,967 | 123,000 | 16,500 |
| `extend_deadline` | 165,273 | 25,080 | 255,000 | 39,000 |
| `keep_alive (2 sponsors)` | 67,984 | 9,555 | 105,000 | 15,000 |
| `get_admin` | 48,269 | 8,042 | 75,000 | 12,750 |
| `get_oracle` | 48,057 | 8,042 | 75,000 | 12,750 |
| `get_treasury` | 47,061 | 8,042 | 75,000 | 12,750 |
| `get_fee_bps` | 47,935 | 8,042 | 75,000 | 12,750 |
| `get_version` | 47,603 | 8,042 | 75,000 | 12,750 |
| `get_max_sponsors` | 48,922 | 8,149 | 75,000 | 12,750 |
| `get_grace_period` | 30,352 | 6,343 | 48,000 | 9,750 |
| `get_bps_denominator` | 30,352 | 6,343 | 48,000 | 9,750 |
| `set_fee_bps` | 121,251 | 21,371 | 187,500 | 33,000 |
| `set_admin` | 136,613 | 28,326 | 210,000 | 43,500 |
| `recover_admin` | 138,772 | 29,756 | 210,000 | 45,000 |
| `set_oracle` | 140,501 | 31,287 | 217,500 | 48,000 |
| `set_treasury` | 118,478 | 24,118 | 180,000 | 37,500 |
| `pause` | 114,466 | 24,261 | 177,000 | 37,500 |
| `is_paused_view` | 45,345 | 8,926 | 70,500 | 14,250 |
| `unpause` | 118,211 | 24,535 | 180,000 | 37,500 |
| `migrate (idempotent)` | 87,593 | 17,430 | 135,000 | 27,000 |
| `refund (permissionless after deadline)` | 320,407 | 47,731 | 675,000 | 120,000 |
| `upgrade` | 126,523 | 26,381 | 240,000 | 52,500 |

### Scaling Curve: `release` by Recipient Count (`MAX_RECIPIENTS = 20`)

| Recipients (`N`) | Measured CPU Instrs | Measured Mem Bytes | Mainnet CPU % (`100M`) | Mainnet Mem % (`40 MiB`) |
|---:|---:|---:|---:|---:|
| 1 | 496,331 | 69,654 | 0.50% | 0.17% |
| 5 | 1,275,853 | 171,782 | 1.28% | 0.41% |
| 10 | 2,341,475 | 308,577 | 2.34% | 0.74% |
| 20 | 4,684,429 | 612,617 | 4.68% | 1.46% |

- **Marginal cost per recipient**: ~220,400 CPU instructions and ~28,600 memory bytes (driven by cross-contract SAC `token_client.transfer` and `PaidTo` event emission per recipient). At `MAX_RECIPIENTS = 20`, `release` uses `< 5%` of the Soroban mainnet per-invocation CPU ceiling.

### Scaling Curve: `refund` by Contributor Count (`MAX_SPONSORS = 20`)

| Contributors (`N`) | Measured CPU Instrs | Measured Mem Bytes | Mainnet CPU % (`100M`) | Mainnet Mem % (`40 MiB`) |
|---:|---:|---:|---:|---:|
| 1 | 317,866 | 44,397 | 0.32% | 0.11% |
| 5 | 1,298,442 | 167,945 | 1.30% | 0.40% |
| 10 | 2,670,856 | 371,655 | 2.67% | 0.89% |
| 20 | 6,014,938 | 943,325 | 6.01% | 2.25% |

- **Marginal cost per contributor**: ~299,800 CPU instructions and ~47,300 memory bytes (driven by reading `DataKey::Contribution(issue_id, i)`, extending its TTL, transferring tokens back to `contribution.sponsor`, and publishing `RefundPaid`). At the default `MAX_SPONSORS = 20`, `refund` uses `6.01%` of the Soroban mainnet CPU budget.

---

## 2. `mergefi-milestones` Baseline & Scaling Curves

### Entrypoint Baselines

| Entrypoint / Scenario | Measured CPU Instrs | Measured Mem Bytes | Regression CPU Ceiling | Regression Mem Ceiling |
|---|---:|---:|---:|---:|
| `initialize` | 94,360 | 11,353 | 142,500 | 18,000 |
| `create_milestone` | 446,929 | 70,128 | 720,000 | 112,500 |
| `contribute` | 518,429 | 76,068 | 780,000 | 120,000 |
| `allocate (initial)` | 195,408 | 32,399 | 375,000 | 67,500 |
| `get_milestone` | 58,289 | 9,153 | 90,000 | 15,000 |
| `get_issue_status` | 43,783 | 7,670 | 82,500 | 14,250 |
| `get_contribution` | 53,169 | 8,412 | 82,500 | 14,250 |
| `get_contributions (2 sponsors)` | 86,271 | 11,670 | 142,500 | 21,000 |
| `deallocate` | 145,281 | 23,016 | 375,000 | 67,500 |
| `keep_alive (2 sponsors, 1 issue)` | 87,125 | 11,590 | 150,000 | 22,500 |
| `get_admin` | 47,121 | 8,246 | 82,500 | 14,250 |
| `get_oracle` | 46,909 | 8,246 | 82,500 | 14,250 |
| `get_treasury` | 45,913 | 8,246 | 82,500 | 14,250 |
| `get_fee_bps` | 46,787 | 8,246 | 82,500 | 14,250 |
| `get_version` | 46,455 | 8,246 | 82,500 | 14,250 |
| `get_max_sponsors` | 47,774 | 8,353 | 82,500 | 14,250 |
| `get_grace_period` | 29,204 | 6,547 | 52,500 | 11,250 |
| `get_bps_denominator` | 29,204 | 6,547 | 52,500 | 11,250 |
| `set_fee_bps` | 121,098 | 22,187 | 195,000 | 37,500 |
| `set_admin` | 141,694 | 29,754 | 217,500 | 48,000 |
| `recover_admin` | 142,439 | 31,184 | 217,500 | 48,000 |
| `set_oracle` | 148,888 | 32,715 | 225,000 | 52,500 |
| `set_treasury` | 123,432 | 24,934 | 187,500 | 42,000 |
| `pause` | 119,267 | 25,077 | 187,500 | 42,000 |
| `is_paused_view` | 46,295 | 9,130 | 75,000 | 15,000 |
| `unpause` | 119,361 | 25,351 | 187,500 | 42,000 |
| `migrate (idempotent)` | 88,499 | 18,042 | 142,500 | 30,000 |
| `cancel_milestone_after_deadline` | 321,249 | 49,763 | 720,000 | 127,500 |
| `upgrade` | 163,845 | 35,127 | 240,000 | 52,500 |

### Scaling Curve: `allocate` and `release_issue` by Prior Allocation Count (`Milestone.allocations` Map Size)

| Prior Allocations (`M`) | `allocate` CPU Instrs | `allocate` Mem Bytes | `release_issue (recipients=1)` CPU Instrs | `release_issue (recipients=1)` Mem Bytes |
|---:|---:|---:|---:|---:|
| 1 | 175,334 | 28,258 | 557,833 | 76,910 |
| 10 | 206,543 | 39,562 | 615,178 | 95,486 |
| 25 | 252,437 | 58,402 | 698,055 | 126,446 |
| 50 | 319,385 | 89,802 | 840,112 | 178,046 |

- **Marginal cost per entry in `Milestone.allocations: Map<u64, i128>`**: ~2,940 CPU instructions and ~1,256 memory bytes on `allocate`, and ~5,760 CPU instructions and ~2,064 memory bytes on `release_issue` (because `Milestone` is deserialized from persistent storage with its inline `allocations` `Map`). Even at 50 issues in a single milestone, `allocate` and `release_issue` stay below `0.85%` of the mainnet CPU budget.

### Scaling Curve: `release_issue` by Recipient Count (`MAX_RECIPIENTS = 20`)

| Recipients (`N`) | Measured CPU Instrs | Measured Mem Bytes | Mainnet CPU % (`100M`) | Mainnet Mem % (`40 MiB`) |
|---:|---:|---:|---:|---:|
| 1 | 557,833 | 76,910 | 0.56% | 0.18% |
| 5 | 1,357,494 | 182,322 | 1.36% | 0.43% |
| 10 | 2,419,944 | 323,222 | 2.42% | 0.77% |
| 20 | 4,813,121 | 635,472 | 4.81% | 1.52% |

### Scaling Curve: `cancel_milestone` by Contributor Count (`MAX_SPONSORS = 20`)

| Contributors (`N`) | Measured CPU Instrs | Measured Mem Bytes | Mainnet CPU % (`100M`) | Mainnet Mem % (`40 MiB`) |
|---:|---:|---:|---:|---:|
| 1 | 325,188 | 45,384 | 0.33% | 0.11% |
| 5 | 1,272,401 | 166,840 | 1.27% | 0.40% |
| 10 | 2,591,245 | 367,935 | 2.59% | 0.88% |
| 20 | 5,828,653 | 934,375 | 5.83% | 2.23% |

---

## 3. `mergefi-maintenance-pool` Baseline & Scaling Curves

### Entrypoint Baselines

| Entrypoint / Scenario | Measured CPU Instrs | Measured Mem Bytes | Regression CPU Ceiling | Regression Mem Ceiling |
|---|---:|---:|---:|---:|
| `initialize` | 84,140 | 10,543 | 142,500 | 18,000 |
| `deposit (initial pool creation)` | 432,387 | 66,459 | 720,000 | 112,500 |
| `get_pool` | 52,779 | 7,917 | 82,500 | 14,250 |
| `get_deposit` | 39,113 | 6,771 | 82,500 | 14,250 |
| `sweep` | 322,849 | 46,135 | 720,000 | 120,000 |
| `get_admin` | 42,306 | 7,117 | 82,500 | 14,250 |
| `get_oracle` | 42,758 | 7,117 | 82,500 | 14,250 |
| `get_treasury` | 42,094 | 7,117 | 82,500 | 14,250 |
| `get_fee_bps` | 41,972 | 7,117 | 82,500 | 14,250 |
| `get_version` | 42,636 | 7,117 | 82,500 | 14,250 |
| `get_bps_denominator` | 27,998 | 5,669 | 52,500 | 11,250 |
| `get_max_deposits` | 27,998 | 5,669 | 52,500 | 11,250 |
| `get_inactivity_window` | 27,998 | 5,669 | 52,500 | 11,250 |
| `set_admin` | 122,376 | 24,496 | 217,500 | 48,000 |
| `recover_admin` | 122,333 | 25,926 | 217,500 | 48,000 |
| `set_oracle` | 127,196 | 27,457 | 225,000 | 52,500 |
| `set_treasury` | 104,754 | 21,308 | 187,500 | 42,000 |
| `pause` | 105,341 | 21,451 | 187,500 | 42,000 |
| `is_paused_view` | 41,794 | 7,865 | 75,000 | 15,000 |
| `unpause` | 107,377 | 21,725 | 187,500 | 42,000 |
| `migrate (idempotent)` | 79,280 | 15,454 | 142,500 | 30,000 |
| `upgrade` | 118,389 | 23,341 | 240,000 | 52,500 |

### Scaling Curve: `deposit`, `keep_alive`, `withdraw`, and `reclaim_deposit` by `deposit_count` (`MAX_DEPOSITS = 200`)

| `deposit_count` (`D`) | `deposit` CPU / Mem | `keep_alive` CPU / Mem | `withdraw (recipients=1)` CPU / Mem | `reclaim_deposit` CPU / Mem |
|---:|---:|---:|---:|---:|
| 1 | 427,581 / 65,539 | 59,017 / 8,099 | 496,735 / 68,881 | 334,737 / 47,586 |
| 10 | 821,001 / 123,487 | 161,917 / 17,279 | 724,814 / 119,641 | 587,809 / 85,764 |
| 25 | 1,526,002 / 224,077 | 405,553 / 37,079 | 1,130,622 / 208,741 | 1,098,299 / 153,894 |
| 50 | 2,909,369 / 404,227 | 988,014 / 82,579 | 1,951,621 / 369,741 | 2,114,499 / 279,944 |

- **Marginal cost per distinct deposit (`D`)**:
  - `deposit`: ~50,600 CPU instructions and ~6,900 memory bytes per existing deposit record (scanning `0..deposit_count` for sponsor deduplication and extending TTLs on all deposit records).
  - `keep_alive`: ~18,900 CPU instructions and ~1,520 memory bytes per deposit record (extending TTL on `DataKey::Deposit(pool_id, i)`).
  - `withdraw` / `reclaim_deposit`: ~29,700–36,300 CPU instructions per deposit record (FIFO deduction across `Deposit` entries and TTL refresh).

### Scaling Curve: `withdraw` by Recipient Count (`MAX_RECIPIENTS = 20`)

| Recipients (`N`) | Measured CPU Instrs | Measured Mem Bytes | Mainnet CPU % (`100M`) | Mainnet Mem % (`40 MiB`) |
|---:|---:|---:|---:|---:|
| 1 | 496,735 | 68,881 | 0.50% | 0.16% |
| 5 | 1,287,129 | 170,901 | 1.29% | 0.41% |
| 10 | 2,350,786 | 307,561 | 2.35% | 0.73% |
| 20 | 4,711,091 | 611,331 | 4.71% | 1.46% |
