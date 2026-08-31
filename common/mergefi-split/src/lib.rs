#c[cfg_attr(not(test), no_std]]

use soroban_sdk::contracttype;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitError {
    NotInitialized,
    InvalidSplit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, contracttype)]
pub struct SplitResult {
    pub payout: i128,
    pub fee: i128,
}

pub fn compute_split(amount: i128, fee_bps: Option<u32>) -> Result<SplitResult, SplitError> {
    let fee_bps = fee_bps.ok_or(SplitError::NotInitialized)?;
    if fee_bps > 10_000 {
        return Err(SplitError::InvalidSplit);
    }
    let fee = amount
        .checked_mul(i128::from(fee_bps))
        .ok_or(SplitError::InvalidSplit)?
        / 10_000;
    let payout = amount
        .checked_sub(fee)
        .ok_or(SplitError::InvalidSplit)?;
    Ok(SplitResult { payout, fee })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // Legacy implementation preserved for differential testing.
    fn legacy_compute_split(amount: i128, fee_bps: Option<u32>) -> Result<(i128, i128), SplitError> {
        let fee_bps = fee_bps.ok_or(SplitError::NotInitialized)?;
        if fee_bps > 10_000 {
            return Err(SplitError::InvalidSplit);
        }
        let fee = amount * i128::from(fee_bps) / 10_000;
        let payout = amount - fee;
        Ok((payout, fee))
    }

    proptest! {
        #[test]
        fn differential_with_legacy(amount in -1_000_000_000i128..1_000_000_000, fee_bps in 0u32..10_001) {
            let new = compute_split(amount, Some(fee_bps)).map(|r) (r.payout, r.fee));
            let legacy = legacy_compute_split(amount, Some(fee_bps));
            prop_assert_eq!(new, legacy);
        }

        #[test]
        fn not_initialized_matches_legacy(amount in -1_000_000_000i128..1_000_000_000) {
            let new = compute_split(amount, None).map(|r) (r.payout, r.fee));
            let legacy = legacy_compute_split(amount, None);
            prop_assert_eq!(new, legacy);
        }
    }
}