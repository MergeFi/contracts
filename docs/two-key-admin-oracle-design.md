# Two-Key Oracle/Admin Separation Design

## Overview

This document outlines the two-key authorization model for MergeFi contracts, separating routine oracle operations (release, withdraw) from high-trust administrative actions (initialize, key rotation, pause/unpause, upgrade).

## Problem Statement

Primary references:
- Stellar contract authorization and account/contract addresses: https://developers.stellar.org/docs/build/guides/auth/contract-authorization
- Stellar contract lifecycle: https://developers.stellar.org/docs/learn/fundamentals/contract-development/contract-lifecycle

Currently, all three contracts use a single `Admin` address for every privileged action:
- Routine operations: `release`, `release_issue`, `withdraw` (called many times daily by `mergefi-backend`)
- High-trust operations: `initialize`, pause/unpause, upgrade (rarely called, require extreme care)

The single `Admin` address is a hot key held by `mergefi-backend` and used to sign routine transactions in response to GitHub webhooks. A compromise of this key gives an attacker:

1. **Immediate damage**: False `release`/`withdraw` calls stealing funds.
2. **Permanent damage**: Pause/unpause and upgrade authority, freezing the contract indefinitely or deploying malicious code.

**Mitigation**: Separate the `Oracle` role (routine operations only) from the `Admin` role (infrastructure operations only), held by different keys with different security postures.

## Design: Two-Role Model

### Roles

#### 1. Oracle

**Authority**: 
- `release(issue_id, recipients)` in escrow
- `release_issue(milestone_id, issue_id, recipients)` in milestones
- `withdraw(pool_id, recipient, amount)` in maintenance-pool

**Characteristics**:
- Routine, high-frequency use (many calls per day).
- Held by `mergefi-backend` as a hot key.
- Used to sign automated transactions from GitHub webhook events.

**Compromise Impact**:
- Attacker can issue false payouts.
- No persistence: funds are stolen, but the contract still functions.

#### 2. Admin

**Authority**:
- `initialize(admin, treasury, fee_bps, max_sponsors, recovery)`
- `pause()` / `unpause()` (Issue #14)
- `upgrade(new_wasm_hash)` (Issue #15)
- `set_oracle(new_oracle)` / `set_admin(new_admin)` (NEW)

**Characteristics**:
- Rarely used (initialization, emergencies, planned upgrades).
- Held by a human or multi-sig address with higher security (cold key, offline storage, or Stellar multisig).
- Used for deliberate, audited infrastructure changes.

**Compromise Impact**:
- Attacker can freeze the contract (pause indefinitely) or deploy malicious code (upgrade).
- Worst-case scenario; requires immediate coordinated response.

### Storage

```rust
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,      // PRIMARY ADMIN: initialize, admin rotation, pause/unpause, upgrade
    Oracle,     // ORACLE: release, release_issue, withdraw (NEW)
    Treasury,
    FeeBps,
    Paused,
    Version,
    // ... existing variants ...
}
```

### Authorization Pattern

Replace the universal `require_admin(&env)?.require_auth()` pattern with role-specific checks:

```rust
fn require_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

fn require_oracle(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Oracle)
}

// For operations that need Admin
pub fn pause(env: Env) -> Result<(), Error> {
    let admin = require_admin(&env).ok_or(Error::NotInitialized)?;
    admin.require_auth();
    env.storage().instance().set(&DataKey::Paused, &true);
    Ok(())
}

// For operations that need Oracle
pub fn release(env: Env, issue_id: u64, recipients: Vec<(Address, u32)>) -> Result<(), Error> {
    let oracle = require_oracle(&env).ok_or(Error::NotInitialized)?;
    oracle.require_auth();
    // ... release logic ...
}
```

### Initialization

Escrow and Milestones contracts:

```rust
pub fn initialize(
    env: Env,
    admin: Address,
    oracle: Address,
    treasury: Address,
    fee_bps: u32,
    max_sponsors: Option<u32>,
    recovery: Option<Address>,
) -> Result<(), Error> {
    admin.require_auth();  // Prevent front-running

    if env.storage().instance().has(&DataKey::Admin) {
        return Err(Error::AlreadyInitialized);
    }
    
    env.storage().instance().set(&DataKey::Admin, &admin);
    env.storage().instance().set(&DataKey::Oracle, &oracle);
    env.storage().instance().set(&DataKey::Treasury, &treasury);
    env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
    // ... rest of initialization ...
    Ok(())
}
```

Maintenance Pool (same pattern):

```rust
pub fn initialize(
    env: Env,
    admin: Address,
    oracle: Address,
    treasury: Address,
    fee_bps: u32,
    recovery: Option<Address>,
) -> Result<(), Error> {
    // ... same pattern ...
}
```

### Key Rotation

New functions allow separate rotation of Admin and Oracle keys:

```rust
/// Rotate the Admin key. Requires the current Admin's authorization.
/// Only the Admin can authorize a new Admin (Admin rotation is self-authorized).
pub fn set_admin(env: Env, new_admin: Address) -> Result<(), Error> {
    let admin = require_admin(&env).ok_or(Error::NotInitialized)?;
    admin.require_auth();  // Current admin approves new admin

    new_admin.require_auth();  // New admin must consent to being named

    env.storage().instance().set(&DataKey::Admin, &new_admin);
    extend_instance_ttl(&env);
    Ok(())
}

/// Rotate the Oracle key. Requires the current Admin's authorization.
/// Note: Oracle cannot rotate itself (only Admin can change Oracle, ensuring
/// the high-trust Admin key maintains control over the hot key).
pub fn set_oracle(env: Env, new_oracle: Address) -> Result<(), Error> {
    let admin = require_admin(&env).ok_or(Error::NotInitialized)?;
    admin.require_auth();  // Only Admin can change Oracle

    new_oracle.require_auth();  // New Oracle must consent

    env.storage().instance().set(&DataKey::Oracle, &new_oracle);
    extend_instance_ttl(&env);
    Ok(())
}
```

### Getters

Expose the current Admin and Oracle addresses:

```rust
pub fn get_admin(env: Env) -> Result<Address, Error> {
    require_admin(&env).ok_or(Error::NotInitialized)
}

pub fn get_oracle(env: Env) -> Result<Address, Error> {
    require_oracle(&env).ok_or(Error::NotInitialized)
}
```

## Rationale: Why Separate Oracle from Admin

### Option 1: Off-Chain Separation (Stellar Multisig Account)

**Approach**: Keep one `Admin` address, but make it a Stellar multisig account (e.g., 2-of-3) that requires multiple signatures for any operation.

**Pros**:
- No contract code changes needed.
- Works with existing tooling (Stellar's native multisig).
- Scales to any signature threshold.

**Cons**:
- Every operation (including routine `release`) requires multisig approval, slowing down the backend.
- Requires multisig setup and coordination overhead.
- Does not achieve the goal of "routine operations don't need high-trust approval."

**Verdict**: Does not fully solve the problem because it doesn't separate the *operational* burden from the *trust* burden.

### Option 2: Contract-Level Two-Role Separation (Recommended)

**Approach**: Add a distinct `Oracle` role with authority for `release`/`withdraw` only. `Admin` retains all high-trust powers.

**Pros**:
- Routine operations can proceed with a hot key without infrastructure overhead.
- High-trust operations remain protected by the secure Admin key.
- Clear separation of concerns: who holds which key reflects its real-world security posture.

**Cons**:
- Requires contract code changes and audit.
- Admin must maintain both keys (more surface for key management mistakes).

**Verdict**: Recommended. The separation achieves the design goal: routine operations don't require high-trust approval, but high-trust operations are protected.

### Hybrid Approach (Future)

Make the `Admin` address itself a Stellar multisig or a Soroban timelock contract, combining the benefits:
- Routine operations use a single hot `Oracle` key.
- High-trust operations require multisig or timelock approval via the `Admin` role.

This is deferred as a future enhancement.

## Migration Path for Already-Deployed Contracts

### Current Testnet Contracts

The three deployed testnet contracts have a single `Admin` and no `Oracle`:

```rust
// Current (before this change)
pub fn release(env: Env, issue_id: u64, recipients: Vec<(Address, u32)>) -> Result<(), Error> {
    let admin = require_admin(&env).ok_or(Error::NotInitialized)?;
    admin.require_auth();
    // ...
}
```

**Options**:

1. **Ignore and redeploy**: Testnet instances are abandoned. Redeploy all three with two-key model baked in.
   - **Pro**: Clean start, no complex migration.
   - **Con**: Testnet data is lost (but testnet is supposed to be ephemeral).

2. **Retrofit via upgrade** (if contracts support it first):
   - Use the upgrade mechanism to add `set_oracle()` and `set_oracle_if_not_set()` functions.
   - An admin call to `set_oracle(oracle_address)` retroactively splits the role.
   - Old `release` calls from the single `Admin` still work; new `release` calls require the split-out `Oracle`.
   - **Pro**: Preserves testnet state and enables testing the upgrade mechanism itself.
   - **Con**: Complex migration logic, higher audit risk.

**Recommendation**: Redeploy testnet with two-key model from the start. Production mainnet will be deployed with both features live. The upgrade mechanism is there for post-deployment fixes, not for retrofitting missing-from-day-1 features.

### Mainnet Deployment

All three contracts are deployed with:

```rust
fn initialize(
    env: Env,
    admin: Address,
    oracle: Address,
    treasury: Address,
    fee_bps: u32,
    max_sponsors: Option<u32>,
    recovery: Option<Address>,
) -> Result<(), Error> {
    // ... requires both admin and oracle addresses ...
}
```

The backend is configured with both keys from day one:
- `admin_key`: Cold/secure key, used only for administrative changes.
- `oracle_key`: Hot key, used for routine `release` calls.

## Testing Strategy

1. **Separate Authorization**: Call `release()` as `admin`; verify it fails. Call `release()` as `oracle`; verify it succeeds.
2. **Separate Key Rotation**: Call `set_oracle()` as `admin`; verify it succeeds. Call `set_oracle()` as `oracle`; verify it fails.
3. **New Oracle Key Works**: Rotate oracle, call `release()` as new oracle; verify it works.
4. **Admin Key Rotation**: Rotate admin, verify old admin can no longer call `set_oracle()` but new admin can.
5. **Backward Compatibility** (if retrofitting): Verify that contracts initialized with a single admin (old testnet) can be upgraded and split via `set_oracle_if_not_set()`.

## Interaction with Other Issues

### Issue #14 (Pause/Unpause)
- **Pause Authority**: Only `Admin` can pause/unpause. `Oracle` cannot.
- Rationale: `Oracle` is a frequent-access hot key; pause should be a deliberate high-trust action.

### Issue #15 (Upgrade)
- **Upgrade Authority**: Only `Admin` can authorize upgrades. `Oracle` cannot.
- Rationale: Upgrade is the highest-risk action; it must remain under high-trust authority.

### Addresses Accessible to Backend

`mergefi-backend` is reconfigured to hold:

1. **Admin Key** (Stellar source account or multisig): Used for `initialize`, `pause`, `unpause`, `upgrade`, key rotations.
   - Low frequency: only at deployment time or during incidents.
   - Higher security posture: offline, cold storage, or multisig-protected.

2. **Oracle Key** (Stellar source account): Used for routine `release`, `release_issue`, `withdraw`.
   - High frequency: many times per day.
   - Standard security: typical hot-key best practices (key rotation, audit logs, etc.).

## Future Work

1. **Guardian Key**: A third role for pause-only authority, enabling a trusted third party to trigger emergency pause without full admin privileges.
2. **Multisig Admin**: Make `Admin` a Stellar multisig or Soroban timelock contract to require multiple approvals for high-trust operations.
3. **Audit Logs**: Emit contract events for every key rotation, pause/unpause, and upgrade, providing an on-chain audit trail.
