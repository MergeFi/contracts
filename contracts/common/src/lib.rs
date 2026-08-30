#![no_std]

use soroban_sdk::{token, Address, Env, IntoVal, Val};

mod split;
pub use split::{compute_split, sort_remainders_desc, Payouts, SplitError};

#[cfg(test)]
mod test_fuzz;

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

/// Maximum allowed single-step fee change (basis points) - Issue #20
/// Prevents accidental or malicious fee spikes (e.g., 2.5% to 99%)
pub const MAX_FEE_CHANGE_BPS: u32 = 500; // 5% maximum change per call

/// Validates a fee change is within acceptable bounds (Issue #20).
/// Ensures new fee is valid (≤100%) and change is ≤5% to prevent spikes.
pub fn validate_fee_change(old_fee: u32, new_fee: u32) -> Result<(), ()> {
    if new_fee as i128 > BPS_DENOMINATOR {
        return Err(());
    }
    
    let delta = if new_fee > old_fee {
        new_fee - old_fee
    } else {
        old_fee - new_fee
    };
    
    if delta > MAX_FEE_CHANGE_BPS {
        return Err(());
    }
    
    Ok(())
}

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

/// Measures the actual balance delta from a token transfer operation,
/// protecting against fee-on-transfer tokens, rebasing tokens, and malicious
/// token contracts (Issue #3).
///
/// Instead of trusting the caller-supplied `amount`, this queries the
/// contract's actual token balance before and after the transfer and returns
/// the real delta. This prevents accounting desync where internal bookkeeping
/// (escrow.amount, milestone.remaining_budget) diverges from the contract's
/// actual holdings.
///
/// # Example
/// ```ignore
/// let actual_received = measure_transfer_delta(
///     &env,
///     &token,
///     &env.current_contract_address(),
///     || {
///         token_client.transfer(&sponsor, &env.current_contract_address(), &amount);
///     },
/// );
/// // Use actual_received for bookkeeping instead of amount
/// ```
pub fn measure_transfer_delta<F>(
    env: &Env,
    token: &Address,
    contract_addr: &Address,
    operation: F,
) -> i128
where
    F: FnOnce(),
{
    let token_client = token::Client::new(env, token);
    let balance_before = token_client.balance(contract_addr);
    operation();
    let balance_after = token_client.balance(contract_addr);
    balance_after - balance_before
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
/// for that residual gap; see `escrow::keep_alive`.
pub fn extend_ttl_for_target<K>(env: &Env, key: &K, target_timestamp: u64)
where
    K: IntoVal<Env, Val>,
{
    let now = env.ledger().timestamp();
    let seconds_until_target = target_timestamp.saturating_sub(now);
    let ledgers_until_target = seconds_until_target / APPROX_SECONDS_PER_LEDGER;

    let current_sequence = env.ledger().sequence() as u64;
    let max_extend_to =
        (env.ledger().max_live_until_ledger() as u64).saturating_sub(current_sequence);

    // Baseline 500_000 mirrors extend_ttl's own extend_to — never do worse
    // than the flat bump every other call site still gets.
    let extend_to = ledgers_until_target.max(500_000).min(max_extend_to);
    let extend_to = u32::try_from(extend_to).unwrap_or(u32::MAX);

    // threshold == extend_to: "ensure at least extend_to ledgers remain,"
    // rather than extend_ttl's "only bother once within threshold of
    // expiring" — a caller invoking this because a far-future target just
    // changed wants the TTL to reflect that immediately, not on a delay.
    env.storage()
        .persistent()
        .extend_ttl(key, extend_to, extend_to);
}
