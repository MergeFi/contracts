//! Basis-point payout splitting with largest-remainder rounding
//! (MergeFi/contracts#16, #142).
//!
//! `compute_split` and its O(n log n) heapsort helpers (`remainder_order_less`,
//! `sift_down_remainder_order`, `sort_remainders_desc`) were duplicated
//! byte-for-byte between `contracts/escrow` and `contracts/milestones` (issue
//! #55 fixed both copies' rounding independently, then #142 flagged that the
//! duplication had grown beyond just `compute_split` itself). This module is
//! the single shared implementation both contracts now call into.
//!
//! Unlike the original per-contract copies, this version takes `fee_bps` as
//! a plain argument instead of reading it from `env.storage()` directly —
//! each contract's `DataKey::FeeBps` storage key and `Error::NotInitialized`
//! variant are contract-specific, so the caller looks its own fee up and
//! passes it in, keeping this function free of any dependency on a
//! particular contract's storage schema or error enum.

use soroban_sdk::{Address, Env, Vec};

const BPS_DENOMINATOR: i128 = 10_000;

/// The computed protocol fee and each recipient's absolute payout amount.
pub struct Payouts {
    pub fee: i128,
    pub shares: Vec<(Address, i128)>,
}

/// Error from [`compute_split`]. Deliberately small and generic — map it to
/// whichever `InvalidSplit`-shaped variant your contract's own `Error` enum
/// already has, e.g. `compute_split(..).map_err(|_| Error::InvalidSplit)?`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitError {
    /// `recipients` was empty, or its basis-point shares don't sum to
    /// exactly `BPS_DENOMINATOR` (10000 = 100%).
    InvalidSplit,
}

/// Validates that basis-point splits sum to exactly 10000 and computes the
/// protocol fee plus each recipient's absolute payout amount.
pub fn compute_split(
    env: &Env,
    total: i128,
    fee_bps: u32,
    recipients: &Vec<(Address, u32)>,
) -> Result<Payouts, SplitError> {
    if recipients.is_empty() {
        return Err(SplitError::InvalidSplit);
    }

    let mut bps_sum: i128 = 0;
    for (_, bps) in recipients.iter() {
        bps_sum += bps as i128;
    }
    if bps_sum != BPS_DENOMINATOR {
        return Err(SplitError::InvalidSplit);
    }

    let fee = total * (fee_bps as i128) / BPS_DENOMINATOR;
    let distributable = total - fee;

    let mut shares: Vec<(Address, i128)> = Vec::new(env);
    let mut order: Vec<(u32, i128, Address)> = Vec::new(env);
    let mut allocated: i128 = 0;

    for (recipient, bps) in recipients.iter() {
        let numerator = distributable * (bps as i128);
        let share = numerator / BPS_DENOMINATOR;
        let remainder = numerator % BPS_DENOMINATOR;
        allocated += share;
        shares.push_back((recipient.clone(), share));
        order.push_back((order.len(), remainder, recipient));
    }

    // Distribute the rounding dust by largest remainder (with the existing
    // address-based tie-break) in O(n log n): sort the (index, remainder,
    // address) records once, then award one unit to each of the first `dust`
    // entries. This is equivalent to a repeated-linear-scan loop, because
    // each award only consumes the selected entry and never changes any
    // other entry's remainder. `dust` is at most `recipients.len() - 1`, so
    // the first `dust` sorted entries always exist.
    let dust = distributable - allocated;
    if dust > 0 {
        sort_remainders_desc(&mut order);
        for k in 0..dust as u32 {
            let (index, _, _) = order.get(k).unwrap();
            let (recipient, share) = shares.get(index).unwrap();
            shares.set(index, (recipient, share + 1));
        }
    }

    Ok(Payouts { fee, shares })
}

/// True if `a` sorts before `b` in largest-remainder order: remainder
/// descending, then address ascending, then original index ascending (which
/// reproduces the address-based tie-break of the previous O(n²) loop).
fn remainder_order_less(a: &(u32, i128, Address), b: &(u32, i128, Address)) -> bool {
    b.1.cmp(&a.1)
        .then_with(|| a.2.cmp(&b.2))
        .then_with(|| a.0.cmp(&b.0))
        == core::cmp::Ordering::Less
}

/// Sifts the element at `start` down a max-heap occupying `[start, end)`,
/// ordering elements by [`remainder_order_less`].
fn sift_down_remainder_order(order: &mut Vec<(u32, i128, Address)>, start: u32, end: u32) {
    let mut root = start;
    loop {
        let mut child = 2 * root + 1;
        if child >= end {
            break;
        }
        if child + 1 < end
            && remainder_order_less(&order.get(child).unwrap(), &order.get(child + 1).unwrap())
        {
            child += 1;
        }
        if remainder_order_less(&order.get(root).unwrap(), &order.get(child).unwrap()) {
            let a = order.get(root).unwrap();
            let b = order.get(child).unwrap();
            order.set(root, b);
            order.set(child, a);
            root = child;
        } else {
            break;
        }
    }
}

/// In-place heapsort of `(index, remainder, address)` records into
/// largest-remainder order. O(n log n) worst case, with no recursion and no
/// heap allocation, so it is safe under `#![no_std]` and only mutates the
/// host-backed `order` through `get`/`set`.
///
/// Exported (not just used internally by [`compute_split`]) because
/// `contracts/milestones`'s `refund_remaining_budget` needs the exact same
/// largest-remainder distribution over a differently-shaped payout
/// (pro-rata refund shares, not a fee split) — this is the shared sorting
/// primitive both cases reduce to, not something specific to `compute_split`.
pub fn sort_remainders_desc(order: &mut Vec<(u32, i128, Address)>) {
    let n = order.len();
    if n < 2 {
        return;
    }

    // Build a max-heap over the whole array.
    let mut start = n / 2;
    loop {
        start -= 1;
        sift_down_remainder_order(order, start, n);
        if start == 0 {
            break;
        }
    }

    // Repeatedly move the largest remaining element to the end of the array,
    // shrinking the heap until the array is sorted ascending by `less`.
    let mut end = n;
    while end > 1 {
        end -= 1;
        let a = order.get(0).unwrap();
        let b = order.get(end).unwrap();
        order.set(0, b);
        order.set(end, a);
        sift_down_remainder_order(order, 0, end);
    }
}
