# Emergency Pause / Circuit-Breaker Mechanism Design

## Overview

This document outlines the emergency pause mechanism for MergeFi contracts to safely halt operations during security incidents or discovered bugs, while preserving user funds and allowing recovery-path operations.

## Problem Statement

Currently, if a bug is discovered in production:
- There is no way to halt problematic operations (e.g., `fund`/`release`/`withdraw`).
- Sponsors can continue depositing into a known-vulnerable contract.
- The only mitigation is for the admin to choose not to call `release`/`allocate`, which doesn't stop permissionless `fund` calls.

A responsible pause mechanism must:
1. Halt risky operations immediately.
2. Preserve legitimate withdrawal/refund paths so users can exit positions.
3. Minimize the new attack surface introduced by the pause lever itself.

## Design Principles

Primary reference for the role checks used by this design:
- Stellar contract authorization: https://developers.stellar.org/docs/build/guides/auth/contract-authorization

1. **Refund and Recovery Paths Remain Available**: A paused contract must still allow sponsor-protective refund/reclaim paths where they exist, so users can recover already-committed funds during an incident.
2. **All Deposits Blocked**: Prevent new funds from entering a known-vulnerable contract (`fund`, `create_milestone`, `deposit`).
3. **Allocations Blocked**: Prevent new commitments (`allocate`, `extend_deadline`).
4. **Admin-Only Pause**: Only the contract admin can trigger pause/unpause to minimize key-count risk.
5. **Clear Errors**: Distinguish "contract is paused" from other errors so client code can handle gracefully.

## Implementation Strategy

### DataKey Addition

Add a `Paused` flag to each contract's storage:

```rust
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Treasury,
    FeeBps,
    Paused,  // NEW: bool, defaults to false
    // ... existing variants ...
}
```

### New Error Variant

Add a `Paused` error (used across all contracts):

```rust
pub enum Error {
    // ...
    ContractPaused = 30,  // NEW
}
```

### Pause/Unpause Functions

Each contract implements:

```rust
/// Admin-gated function to pause the contract.
/// When paused, operations like fund/allocate/deposit are blocked,
/// but refund/withdraw remain available for users to exit positions.
pub fn pause(env: Env) -> Result<(), Error> {
    let admin = require_admin(&env).ok_or(Error::NotInitialized)?;
    admin.require_auth();

    env.storage().instance().set(&DataKey::Paused, &true);
    extend_instance_ttl(&env);
    Ok(())
}

/// Admin-gated function to unpause the contract.
pub fn unpause(env: Env) -> Result<(), Error> {
    let admin = require_admin(&env).ok_or(Error::NotInitialized)?;
    admin.require_auth();

    env.storage().instance().set(&DataKey::Paused, &false);
    extend_instance_ttl(&env);
    Ok(())
}

fn is_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}
```

### Guarded Operations

#### Escrow Contract

**Blocked when paused:**
- `fund()` — prevent new escrows
- `contribute()` — prevent new contributions
- `release()` — prevent payouts (debatable; see analysis below)
- `extend_deadline()` — prevent extending paused escrows

**Allowed when paused:**
- `refund()` — users can recover funds
- `get_*()` — read operations never blocked

```rust
pub fn fund(env: Env, issue_id: u64, sponsor: Address, token: Address, 
           amount: i128, deadline: u64, target: Option<i128>) -> Result<(), Error> {
    if is_paused(&env) {
        return Err(Error::ContractPaused);
    }
    // ... rest of fund logic ...
}

pub fn refund(env: Env, issue_id: u64) -> Result<(), Error> {
    // No pause check — refunds always allowed
    // ... rest of refund logic ...
}
```

#### Milestones Contract

**Blocked when paused:**
- `create_milestone()` — prevent new milestones
- `contribute()` — prevent new contributions
- `allocate()` — prevent allocations
- `release_issue()` — prevent payouts

**Allowed when paused:**
- `cancel_milestone()` — refund remaining funds to contributors

```rust
pub fn cancel_milestone(env: Env, milestone_id: u64) -> Result<(), Error> {
    // No pause check — cancellations (refunds) always allowed
    // ... rest of cancel logic ...
}
```

#### Maintenance Pool Contract

**Blocked when paused:**
- `deposit()` — prevent new deposits
- `withdraw()` — prevent payouts

**Allowed when paused:**
- `reclaim_deposit()` — sponsor recovery after the inactivity window
- `sweep()` — admin surplus recovery for accidental direct transfers
- `get_*()` — read operations

**Note**: Maintenance pools do not have an immediate permissionless refund of active balances. They do have `reclaim_deposit()` after `INACTIVITY_WINDOW`, which remains callable while paused. Routine oracle `withdraw()` is blocked because it is the exact high-frequency payout path that may need to be stopped during an incident.

### Query Function

Add a getter to check pause status:

```rust
pub fn is_paused(env: Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}
```

This lets clients check before attempting operations and provide UX feedback.

## Rationale: Why Release/Withdraw Can Be Blocked

**Intuition**: Release/withdraw are *admin-initiated* actions, not sponsor-initiated. If the admin discovers a bug and pauses, the admin can choose when to resume operations and make payouts safely.

**Counter-argument**: If the backend is compromised, a compromised admin might issue false `release` calls before an honest party discovers the compromise. Blocking `release` during a pause prevents this.

**Decision**: Block `release` and `allocate` when paused. Sponsors/users can still exit via refund/cancel. The assumption is that incident response is *coordinated* — when a pause is triggered, the team is already in communication and will resume carefully and audited.

**Timelock Future Work**: For extremely high-trust scenarios, a future upgrade could add a timelock so that pause/unpause aren't instantaneous (e.g., "pause takes effect in 1 hour", allowing a 1-hour window to pull funds before enforcement).

## Two-Key Interaction (Issue #13)

If a two-key oracle/admin model is implemented:

- **Pause Authority**: The primary `Admin` key only (not the `Oracle` key), as pause is a high-impact control.
- **Unpause Authority**: Same `Admin` key.
- **Guardian Key** (future): A separate guardian could have pause-only authority, enabling a trusted third party to pause without full admin privileges.

## Testing Strategy

1. **Pause State Persists**: Pause the contract, verify subsequent calls see `is_paused_view() == true`.
2. **Blocked Operations Fail**: Attempt `fund`/`allocate`/`deposit` while paused; verify they return `ContractPaused`.
3. **Allowed Operations Succeed**: Attempt `refund`/`cancel_milestone`/`reclaim_deposit` while paused; verify they succeed when their normal preconditions are met.
4. **Unpause Restores Normal State**: Unpause, verify `fund` works again.
5. **Unauthorized Pause Fails**: Attempt `pause()` as non-admin; verify it returns `Unauthorized`.
6. **Read Operations Always Work**: `get_escrow()`, `get_deposit()`, etc. work regardless of pause state.

## Operational Runbook

### When to Pause

1. A security researcher reports a potential exploit.
2. An internal audit discovers a bug or logical flaw.
3. Unusual on-chain activity suggests an attack (e.g., rapid double-funding attempts).

### Pause Procedure

1. Admin calls `pause()` on the affected contract.
2. The backend immediately stops initiating new `release`/`allocate`/`withdraw` calls.
3. Users are notified (off-chain) that the contract is paused. Escrow contributors can recover through `refund` once eligible, milestone funds can be returned through `cancel_milestone`, and maintenance-pool sponsors retain the inactivity-window `reclaim_deposit` path.
4. The team audits the bug, prepares a fix, and deploys a new wasm via the upgrade mechanism.

### Unpause Procedure

1. New wasm is live and verified.
2. Admin calls `unpause()`.
3. Normal operations resume.
4. Users are notified that the contract is operational again.

## Limitations and Future Work

1. **No Pause-on-Threshold**: This design is manual admin-triggered only. Future iterations could add automatic triggers (e.g., "pause if more than $X is withdrawn in an hour"), but this is out of scope.
2. **Maintenance Pool Gap**: Maintenance pools have no permissionless refund. Sponsors should be made aware that a pause is more disruptive for pools than for escrows/milestones.
3. **Timelock**: A future enhancement could add a delay between pause and enforcement, or between unpause and restoration, giving users time to react.
4. **Multi-Admin Approval**: A future enhancement could require multiple admins to approve a pause, reducing the risk of a single-key compromise being able to freeze funds.

## Cross-Contract Considerations

- Each of the three contracts implements pause independently (no shared registry).
- If a bug affects all three, the admin must call `pause()` on each one.
- Backend must be hardened to retry gracefully and alert operators when a contract is paused.
