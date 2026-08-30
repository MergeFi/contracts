#![no_std]

use soroban_sdk::{Address, Env, IntoVal, Val};

mod split;
pub use split::{compute_split, sort_remainders_desc, Payouts, SplitError};

/// Trait to identify the Admin key for a contract's DataKey enum
pub trait AdminKey {
    fn admin_key() -> Self;
}

pub fn require_admin<K>(env: &Env) -> Option<Address>
where
    K: AdminKey + IntoVal<Env, Val>,
{
    env.storage().instance().get(&K::admin_key())
}

/// Trait to identify the Oracle key for a contract's DataKey enum.
/// Oracle is authorized for routine operations like release/withdraw.
pub trait OracleKey {
    fn oracle_key() -> Self;
}

pub fn require_oracle<K>(env: &Env) -> Option<Address>
where
    K: OracleKey + IntoVal<Env, Val>,
{
    env.storage().instance().get(&K::oracle_key())
}

/// Trait to identify the Treasury key for a contract's DataKey enum
pub trait TreasuryKey {
    fn treasury_key() -> Self;
}

pub fn require_treasury<K>(env: &Env) -> Option<Address>
where
    K: TreasuryKey + IntoVal<Env, Val>,
{
    env.storage().instance().get(&K::treasury_key())
}

/// Trait to identify the FeeBps key for a contract's DataKey enum
pub trait FeeBpsKey {
    fn fee_bps_key() -> Self;
}

pub fn get_fee_bps<K>(env: &Env) -> Option<u32>
where
    K: FeeBpsKey + IntoVal<Env, Val>,
{
    env.storage().instance().get(&K::fee_bps_key())
}
/// Shared denominators and defaults used across multiple contracts.
pub const BPS_DENOMINATOR: i128 = 10_000;
pub const MAX_SPONSORS: u32 = 20;

pub fn extend_ttl<K>(env: &Env, key: &K)
where
    K: IntoVal<Env, Val>,
{
    env.storage().persistent().extend_ttl(key, 100_000, 500_000);
}

/// Stellar's approximate ledger close time, used to convert a duration into
/// an approximate ledger count. Not a protocol constant — the network's
/// actual average could drift — but the order of magnitude is what matters
/// here, not sub-day precision. See `extend_ttl_for_target`'s docs.
const APPROX_SECONDS_PER_LEDGER: u64 = 5;

/// Soroban's real ceiling on how many ledgers a single `extend_ttl` call can
/// add ahead of the *current* sequence (`max_live_until_ledger() - sequence()`),
/// as a `u32` extend-to value. Shared by every "scale the TTL bump" helper
/// below so none of them can push a call past what the network actually
/// allows in one shot (MergeFi/contracts#56, MergeFi/contracts#11).
fn max_extend_to(env: &Env) -> u32 {
    let current_sequence = env.ledger().sequence() as u64;
    let ceiling = (env.ledger().max_live_until_ledger() as u64).saturating_sub(current_sequence);
    u32::try_from(ceiling).unwrap_or(u32::MAX)
}

/// Converts `target_timestamp - now` into an approximate ledger count at
/// `APPROX_SECONDS_PER_LEDGER`, capped at `max_extend_to`. A
/// `target_timestamp` at or before `now`, or one so far out it would
/// resolve to fewer ledgers than the existing flat bump, still resolves to
/// at least that flat 500_000 baseline — this only ever extends *further*
/// than `extend_ttl` would, never less.
fn scaled_extend_to(env: &Env, target_timestamp: u64) -> u32 {
    let now = env.ledger().timestamp();
    let seconds_until_target = target_timestamp.saturating_sub(now);
    let ledgers_until_target = seconds_until_target / APPROX_SECONDS_PER_LEDGER;

    // Baseline 500_000 mirrors extend_ttl's own extend_to — never do worse
    // than the flat bump every other call site still gets.
    let extend_to = ledgers_until_target.max(500_000);
    let extend_to = u32::try_from(extend_to).unwrap_or(u32::MAX);
    extend_to.min(max_extend_to(env))
}

/// Extends a persistent entry's TTL to (approximately) survive until
/// `target_timestamp`, not just the fixed ~29-day (`500_000`-ledger) bump
/// `extend_ttl` always applies regardless of context.
///
/// `extend_ttl`'s flat bump creates a real gap wherever a caller can push a
/// domain deadline arbitrarily far into the future (e.g. escrow's
/// `extend_deadline`): the *deadline* moves, but the record's actual
/// on-chain survivability doesn't move with it, so a far-future deadline
/// silently buys nothing beyond the same ~29 days every other call gets
/// (MergeFi/contracts#56).
///
/// This converts `target_timestamp - now` into an approximate ledger count
/// at `APPROX_SECONDS_PER_LEDGER` and extends to that many ledgers, capped
/// at `env.ledger().max_live_until_ledger()` — Soroban's actual, real-time-
/// queryable ceiling on a persistent entry's TTL (~1 year on a network
/// configured like this SDK's own test defaults). A `target_timestamp` at or
/// before `now`, or one so far out it would resolve to fewer ledgers than
/// the existing flat bump, still gets at least that flat bump — this only
/// ever extends *further* than `extend_ttl` would, never less.
///
/// Even at the network's actual TTL ceiling, a sufficiently far-future
/// `target_timestamp` (e.g. multiple years out) still can't be fully
/// covered by a single call — nothing can, that ceiling is a real Soroban
/// limit, not an implementation shortcut. A permissionless "keep alive"
/// entry point that re-calls this periodically is the intended mitigation
/// for that residual gap; see `escrow::keep_alive` / `milestones::keep_alive`.
pub fn extend_ttl_for_target<K>(env: &Env, key: &K, target_timestamp: u64)
where
    K: IntoVal<Env, Val>,
{
    let extend_to = scaled_extend_to(env, target_timestamp);
    // threshold == extend_to: "ensure at least extend_to ledgers remain,"
    // rather than extend_ttl's "only bother once within threshold of
    // expiring" — a caller invoking this because a far-future target just
    // changed wants the TTL to reflect that immediately, not on a delay.
    env.storage()
        .persistent()
        .extend_ttl(key, extend_to, extend_to);
}

/// Same scaling as `extend_ttl_for_target`, applied to the calling
/// contract's *instance* storage instead of a keyed persistent entry.
/// Instance storage (Admin/Treasury/FeeBps/...) has no domain deadline of
/// its own, but a record-level deadline-scaled extension is only useful if
/// the instance storage backing the whole contract survives at least as
/// long — otherwise every other record in the contract becomes unreachable
/// once instance storage archives, regardless of any individual record's
/// own TTL (MergeFi/contracts#11).
pub fn extend_instance_ttl_for_target(env: &Env, target_timestamp: u64) {
    let extend_to = scaled_extend_to(env, target_timestamp);
    env.storage().instance().extend_ttl(extend_to, extend_to);
}

/// Extends a persistent entry's TTL as far as Soroban's own persistent-entry
/// ceiling allows in a single call (`max_live_until_ledger()`), for records
/// with no natural deadline to scale toward at all — e.g.
/// `maintenance-pool`, which is explicitly open-ended/recurring rather than
/// tied to a bounded bounty or release-cycle deadline (MergeFi/contracts#11).
/// Where `extend_ttl_for_target` scales toward a *known* future point, this
/// is for records where the honest answer to "how far out" is
/// "indefinitely" — so it always asks for the maximum a single call can
/// grant.
pub fn extend_ttl_to_max<K>(env: &Env, key: &K)
where
    K: IntoVal<Env, Val>,
{
    let extend_to = max_extend_to(env);
    env.storage()
        .persistent()
        .extend_ttl(key, extend_to, extend_to);
}

/// `extend_ttl_to_max`, applied to the calling contract's instance storage.
pub fn extend_instance_ttl_to_max(env: &Env) {
    let extend_to = max_extend_to(env);
    env.storage().instance().extend_ttl(extend_to, extend_to);
}
