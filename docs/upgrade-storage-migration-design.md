# Contract Upgrade and Storage Migration Design

## Overview

This document outlines the upgrade mechanism, storage versioning strategy, and migration path for MergeFi contracts to support safe in-place wasm upgrades via `env.deployer().update_current_contract_wasm()`.

## Problem Statement

MergeFi contracts currently have no upgrade mechanism. Deploying a new wasm version creates a new contract address with empty storage, stranding existing on-chain state. This is unacceptable for contracts holding live user funds.

## Soroban Upgrade Mechanism

Primary references:
- Stellar contract lifecycle and deployment: https://developers.stellar.org/docs/learn/fundamentals/contract-development/contract-lifecycle
- Stellar contract authorization: https://developers.stellar.org/docs/build/guides/auth/contract-authorization
- Stellar state archival and TTL: https://developers.stellar.org/docs/learn/fundamentals/contract-development/storage/state-archival

### How It Works

- `env.deployer().update_current_contract_wasm(new_wasm_hash)` upgrades code *in place* at the same address while **preserving all existing storage**.
- Requires the calling contract to explicitly implement an upgrade function with appropriate authorization.
- Existing `#[contracttype]` structs and storage entries remain accessible in the upgraded code, as long as field layouts don't change incompatibly.

### Key Guarantees and Limitations

- **Storage preservation**: All persistent and instance storage entries survive the upgrade.
- **Type safety concern**: If a struct's field order or types change, old data becomes inaccessible/corrupt. Soroban has no automatic migration mechanism.
- **Authorization**: The upgrade function must explicitly call `require_auth()` on the authorized caller (the admin/upgrade key).

### Retrofitting Already-Deployed Contracts

**Current Status**: The three deployed testnet contracts have **no upgrade function and cannot support in-place upgrades**. Any new code requires a full redeploy.

**Mitigation**: Before mainnet, redeploy all three contracts with upgrade support baked in from the start (no need to retrofit the testnet instances; they can be abandoned).

## Storage Versioning Strategy

We implement a versioning convention to safely detect and migrate old-shape data when struct layouts change.

### Approach

1. **Version Key**: Add a `DataKey::Version` entry storing the current contract version.
2. **Default Handling**: On contract interaction, check the stored version against the current code version.
3. **Lazy Migration**: If old data is detected, migrate it transparently on first read (if layout changes are backward-compatible) or fail with a clear error.
4. **Explicit Migration**: Provide an admin-callable `migrate_storage()` function for complex layout changes requiring a full pass over all affected entries.

### Implementation

```rust
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Treasury,
    FeeBps,
    Version,       // NEW: current contract version
    Paused,        // NEW: pause flag
    Escrow(u64),
    // ... other variants ...
}

// Version tracking
const CONTRACT_VERSION: u32 = 1;

fn get_contract_version(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::Version)
        .unwrap_or(0)
}

fn set_contract_version(env: &Env, version: u32) {
    env.storage().instance().set(&DataKey::Version, &version);
}
```

### Migration Scenarios

**Scenario 1: Adding an optional field to a struct**
- Old data: `Escrow { token, amount, status, created_at, deadline, contributor_count }`
- New data: `Escrow { token, amount, status, created_at, deadline, contributor_count, target, version }`
- Migration: Set new fields to defaults (`target: None`, `version: 1`) on first read.
- **Safe because**: old `Deposit` data still deserializes; new fields fill in defaults.

**Scenario 2: Changing the split accounting structure (e.g., issue #64 redesign)**
- Old data: `Map<(issue_id, recipient), amount>`
- New data: `Map<(issue_id, version), V1AllocationData { ... }>`
- Migration: Requires explicit `migrate_storage()` call to walk all entries and rewrite them.
- **Not automatic**: The function runs only when an admin explicitly calls it, after thorough review.

## Upgrade Function Specification

```rust
/// Admin-gated upgrade function. Requires the stored `Admin` address's authorization.
/// Calls `env.deployer().update_current_contract_wasm(new_wasm_hash)` to upgrade
/// the contract code in place while preserving storage.
///
/// After upgrade, the new code should immediately check the stored `Version` and
/// perform any necessary lazy migrations if old data is detected.
pub fn upgrade(
    env: Env,
    new_wasm_hash: soroban_sdk::BytesN<32>,
) -> Result<(), Error> {
    let admin = require_admin(&env).ok_or(Error::NotInitialized)?;
    admin.require_auth();

    env.deployer().update_current_contract_wasm(new_wasm_hash);
    set_contract_version(&env, CONTRACT_VERSION);
    extend_instance_ttl(&env);

    Ok(())
}
```

## Pause Mechanism Integration

The pause mechanism (Issue #14) can coexist with the upgrade mechanism:

1. **Pause Before Upgrade**: If a bug is detected, the admin can pause the contract to halt user interactions.
2. **Upgrade Paused Contract**: A paused contract can still be upgraded (the upgrade itself doesn't check pause status).
3. **Post-Upgrade Resume**: After upgrade validation, the admin unpauses to resume normal operation.

## Integration with Two-Key Admin Model (Issue #13)

If a two-key oracle/admin model is implemented:

- **Upgrade Authority**: Only the primary `Admin` key can authorize upgrades (not the `Oracle` key), as upgrades are high-risk.
- **Pause Authority**: The `Admin` or a dedicated `Guardian` key can trigger pause (see Issue #14).

## Testing Strategy

1. **Upgrade Without Data**: Deploy a new version, verify the contract initializes cleanly.
2. **Upgrade With Data**: Upgrade a contract with existing escrows/milestones/deposits, verify data survives and remains accessible.
3. **Version Tracking**: Verify that `get_contract_version()` returns 0 before first upgrade, and increments correctly.
4. **Lazy Migration**: Add optional fields to a struct, verify old entries still deserialize and new fields fill with defaults.
5. **Explicit Migration**: Trigger `migrate_storage()` on a contract with outdated entries, verify correctness of rewritten entries.

## Deployment and Rollout

### Testnet (Current)

1. The three currently-deployed testnet contracts **cannot be upgraded in place** (no upgrade function).
2. **Recommendation**: Do not invest in retrofitting them. Instead, redeploy all three with upgrade support before mainnet testing.
3. Any data on the old testnet instances is abandoned; `mergefi-backend` migrates to the new contract IDs.

### Mainnet (Pre-Launch)

1. All three contracts deployed with `version = 0` initially and upgrade support built in.
2. A "final audit" can proceed against the live mainnet contracts, without risk of unupgradeable bugs trapping funds.
3. If auditors find a critical issue, an emergency pause and upgrade cycle can execute safely.

## Rollback Considerations

Soroban does not support "downgrading" a contract (reverting to an older wasm hash). However:

- A **paused** contract can remain frozen while the team prepares a fix.
- Once a fix is verified, upgrade to the corrected version.
- Rollback via upgrade to a version built *before* the bug is possible, but requires verifying that the old version's code is safe for the current state (migration backward is often harder than forward).

## Backward Compatibility

- **Existing testnet contracts**: No action required; they will be abandoned and redeployed.
- **Existing mainnet contracts (if any, before this feature lands)**: A one-time "activate upgrade support" redeploy is required. This is why the recommendation is to land this feature *before* mainnet launch.

## Future Work

1. **Atomic Versioning Across Contracts**: If `mergefi-backend` needs to coordinate upgrades across all three contracts, consider a shared version registry (a tiny separate contract or a backend cache).
2. **Audit Trail**: Log upgrade events (emit Soroban contract events) to provide an on-chain history of all upgrades.
3. **Timelock**: Consider an upgrade timelock (see Issue #13's two-key model) where an upgrade is announced and scheduled for a future block, allowing users to exit positions before a potentially-risky upgrade.
