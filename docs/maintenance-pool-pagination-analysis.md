# Bounded/Paginated Access Pattern for Maintenance Pool Deposit History

## Overview

This document analyzes the current and proposed access patterns for querying deposit history in the maintenance-pool contract, evaluating whether pagination or other access patterns are needed given the contract's lifecycle and backend integration model.

## Problem Statement

The `maintenance-pool::deposit` function writes one `Deposit` entry per call, indexed by `DataKey::Deposit(pool_id, index)` where `index` monotonically increments (never reset).

**Current access pattern**:
- `get_deposit(pool_id, index)` — read a single deposit by index
- No enumeration function — no way to list all deposits for a pool
- No pagination — no way to efficiently fetch a range of deposits

**Concern**: Maintenance pools are described as receiving *repeated* deposits over years (potentially thousands). Some scenarios might require off-chain clients to reconstruct the full deposit history:

1. **Audit/Compliance**: "Show me every deposit and withdrawal for pool X in the last 30 days."
2. **UI/Dashboard**: "Display the contribution history for this pool."
3. **Backend Sync**: `mergefi-backend` needs to verify its off-chain database against on-chain state.

**Questions raised by the issue**:

1. Does `mergefi-backend` actually need full deposit history from the contract, or does it index deposits via contract events?
2. Is the current one-at-a-time-by-index interface a latent scaling problem (n RPC calls to fetch n deposits)?
3. Does the ever-growing persistent storage have cost or liveness implications?

## Investigation: Soroban Storage Economics

Primary references:
- Stellar state archival and TTL: https://developers.stellar.org/docs/learn/fundamentals/contract-development/storage/state-archival
- Stellar storage strategies: https://developers.stellar.org/docs/build/guides/storage/storage-strategies

### Persistent Entry Costs

**Storage Pricing**:
- Soroban charges for ledger-entry size and TTL extension through rent/restore mechanics rather than through a fixed "number of rows" tax.
- Each deposit is a separate persistent ledger entry, so a pool with many deposits has many entries to extend or restore independently.
- The exact stroop cost is network-configuration dependent; clients should estimate it through simulation instead of hardcoding a constant.

**TTL Model**:
- Each persistent entry has its own TTL.
- To keep an entry live, the contract must call `extend_ttl()` before it expires.
- If TTL expires, the entry becomes archived and requires restoration before normal reads can succeed.
- `maintenance-pool::keep_alive(pool_id)` refreshes both the parent pool entry and each deposit sub-record.

**Growth Scenario**:
- A maintenance pool receives 10 deposits per month, 120 per year, 1200 over 10 years.
- Each `Deposit` entry (sponsor, amount, timestamp) is ~100 bytes on-chain.
- 1200 entries means 1200 independent deposit TTLs plus the parent pool TTL.
- That is operationally manageable, but every "refresh full history" pass is O(n) in the number of deposits.

**Network Impact**:
- This is not a correctness bottleneck at the expected early scale.
- It is a maintenance-cost and RPC-efficiency concern, especially if a backend tries to rebuild history by repeatedly calling `get_deposit(pool_id, i)`.

## Investigation: MergeFi Backend Integration

### Described Architecture (per README)

> "Backend integration" section describes a webhook-driven model where:
> 1. `mergefi-backend` watches GitHub webhooks.
> 2. On relevant events (PR merged, issue closed), it calls `release`/`refund`/`withdraw` on contracts.
> 3. It emits events (logs and persistence, not detailed in current README).

### Event-Driven Indexing

A *future* issue on the Roadmap suggests adding contract events. Once implemented:

```rust
pub fn deposit(env: Env, pool_id: u64, sponsor: Address, token: Address, amount: i128) {
    // ... deposit logic ...
    
    // NEW: Emit event for off-chain indexing
    env.emit_contract("deposit_created", (&pool_id, &sponsor, &amount, &env.ledger().timestamp()));
}
```

With event emission:
- `mergefi-backend` listens to contract events from the ledger (via Soroban RPC).
- It maintains its own database of deposits (indexed by pool_id, sponsor, timestamp, etc.).
- It never needs to call `get_deposit()` to reconstruct history.
- The on-chain `get_deposit()` interface is a fallback for audit/verification, not a primary sync mechanism.

### Current Reality (Pre-Events)

Without events, `mergefi-backend` must:
- Query each pool's `get_pool()` to fetch `deposit_count`.
- Call `get_deposit(pool_id, i)` for each `i` from 0 to `deposit_count - 1` (n RPC calls).
- Rebuild the full history in its own database.

This is an n-call problem *today*, but:
1. It only runs on startup/sync, not on every transaction.
2. With events, the problem is eliminated entirely.
3. For reasonable pool sizes (hundreds to low thousands of deposits), the sync latency is acceptable.

## Recommendation

### Conclusion: No Code Change Needed (For Now)

Based on the investigation:

1. **Event-Driven Architecture Wins**: Once the Roadmap's "emit contract events" issue is implemented, `get_deposit()` becomes a fallback-only interface. The n-call sync problem is solved at the source.

2. **Current Scaling Is Acceptable**: Thousands of deposits per pool is within Soroban's persistent storage comfort zone. No cost/liveness emergency exists today.

3. **Simplicity Wins**: Adding pagination (`get_deposits(pool_id, start, limit)`) is straightforward but adds code complexity and test surface with minimal benefit if events are the real sync mechanism.

4. **Defer Until Events Land**: Once event emission is implemented and the backend is re-architected around it, reconsider whether *any* paginated getter is needed (answer: probably not).

### If Pagination Were Needed (Reference Implementation)

Should a future iteration decide pagination is necessary, here's the safe way to add it:

```rust
/// Fetch a range of deposits for a pool, paginated.
/// 
/// Returns up to `limit` deposits, starting from `start_index`.
/// If `start_index` >= `pool.deposit_count`, returns empty.
/// 
/// Note: This is a view function (read-only, no fees). Suitable for
/// off-chain audit/UI queries. Prefer contract events for backend sync.
pub fn get_deposits(
    env: Env,
    pool_id: u64,
    start_index: u32,
    limit: u32,
) -> Result<Vec<Deposit>, Error> {
    let pool_key = DataKey::Pool(pool_id);
    let pool: MaintenancePool = env
        .storage()
        .persistent()
        .get(&pool_key)
        .ok_or(Error::PoolNotFound)?;

    let mut deposits = Vec::new();
    let end_index = start_index.saturating_add(limit).min(pool.deposit_count);

    for i in start_index..end_index {
        let deposit_key = DataKey::Deposit(pool_id, i);
        if let Some(deposit) = env.storage().persistent().get::<_, Deposit>(&deposit_key) {
            deposits.push(deposit);
        }
    }

    Ok(deposits)
}

/// Get the total deposit count for a pool (helper for pagination).
pub fn get_deposit_count(env: Env, pool_id: u64) -> Result<u32, Error> {
    let pool_key = DataKey::Pool(pool_id);
    let pool: MaintenancePool = env
        .storage()
        .persistent()
        .get(&pool_key)
        .ok_or(Error::PoolNotFound)?;
    Ok(pool.deposit_count)
}
```

**Tests for paginated access**:
- Create 100 deposits for a pool.
- Call `get_deposits(pool_id, 0, 10)`, verify first 10 returned.
- Call `get_deposits(pool_id, 10, 10)`, verify next 10 returned.
- Call `get_deposits(pool_id, 90, 50)` (limit > remaining), verify last 10 returned.
- Call `get_deposits(pool_id, 100, 10)` (start > count), verify empty vec returned.

## Summary

| Aspect | Finding |
|--------|---------|
| **Storage cost** | Entry-count growth increases TTL/restore work; estimate through simulation, no fixed cost assumed |
| **Backend sync** | Event-driven architecture (future) makes n-call sync moot |
| **On-chain enumeration** | Not needed if events are primary sync mechanism |
| **UX audit trail** | Off-chain database is source of truth; on-chain is fallback |
| **Recommended action** | **No code change required**. Land the events issue first; if pagination is still needed post-events, add it then. |

## Cross-References

- Roadmap: "Emit contract events" — once landed, re-evaluate deposit access patterns.
- README: "Backend integration" section should be updated to clarify event-driven sync once events are implemented.
