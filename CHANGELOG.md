# Changelog

All notable behavior changes to the Soroban contracts in this repository are
documented here, so a downstream integrator (`mergefi-backend` in
particular) has one place to read what changed between two versions of the
deployed contracts, instead of reading every closed issue individually.

Format loosely follows [Keep a Changelog](https://keepachangelog.com/).
New entries should be added alongside the PR that introduces the behavior
change, not backfilled later.

## [Unreleased]

### Added
- `CODEOWNERS`, pinning `contracts/`, `docs/` and `scripts/` review to the
  maintainer for automatic reviewer assignment on the high volume of
  external bounty-program PRs this repo receives (#301, cross-ref #117).
- `rust-toolchain.toml`, pinning the exact Rust toolchain `rustup`
  auto-installs in this directory to match what CI uses, rather than
  leaving a contributor's local build on whatever they happen to have
  installed (#303, cross-ref #119).
- This `CHANGELOG.md` (#302, cross-ref #118).

## Historical entries (seeded retroactively)

### Crowdfunding support in `escrow` (#57, #58)
Escrows gained multi-sponsor crowdfunding: several sponsors can contribute
toward one escrow's funding target instead of a single sponsor funding it
outright, with a permissionless refund path once the deadline plus
`GRACE_PERIOD` has passed and the target wasn't met.

### Pause / circuit-breaker mechanism (#14)
`escrow`, `maintenance-pool`, and `milestones` each gained `pause`/`unpause`
entrypoints. State-mutating operations across all three contracts are
blocked while paused, giving an admin a circuit breaker to halt activity
during an incident without needing to upgrade or redeploy.

### Fee-update bounds (#20)
Added `validate_fee_change` (`contracts/common`): a fee change is capped at
`MAX_FEE_CHANGE_BPS` (5 percentage points) per call, and the resulting fee
can never exceed `BPS_DENOMINATOR` (100%). Prevents an accidental or
malicious single-step fee spike (e.g. 2.5% to 99%).

### `GRACE_PERIOD` for permissionless refunds and TTL scaling (#49, #56)
Introduced `GRACE_PERIOD` (14 days) in `escrow`: contributors can only
permissionlessly refund an unmet crowdfund after `deadline + GRACE_PERIOD`
has passed, giving the sponsor/maintainer a window to act first.
`extend_deadline`/`keep_alive` storage TTL scaling was corrected to extend
against the full `deadline + GRACE_PERIOD` window rather than the deadline
alone, so a call can't be made to cover unlimited future time or fall short
of covering the actual refund window (#56).

### Admin rotation (`set_admin`/`get_admin`)
`escrow` gained `set_admin`/`get_admin` entrypoints for rotating the
contract's stored admin address without a redeploy.
