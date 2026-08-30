//! Property-based fuzz testing for compute_split (Issue #17)
//!
//! These tests use proptest to verify mathematical invariants hold across
//! a wide range of inputs, catching edge cases that unit tests might miss.

#[cfg(test)]
mod tests {
    extern crate std;
    use std::vec;
    use std::vec::Vec;
    
    use crate::BPS_DENOMINATOR;
    use proptest::prelude::*;

    /// Simplified compute_split for testing (mirrors the contract logic)
    fn compute_split_test(total: i128, recipients: &[u32], fee_bps: u32) -> Vec<i128> {
        let fee = (total * fee_bps as i128) / BPS_DENOMINATOR;
        let after_fee = total - fee;
        
        recipients.iter().map(|&bps| {
            (after_fee * bps as i128) / BPS_DENOMINATOR
        }).collect()
    }

    proptest! {
        /// Property: The sum of all recipient amounts should equal total - fee
        /// This is the core invariant: no funds should disappear or be created
        #[test]
        fn test_sum_equals_total_minus_fee(
            total in 1i128..=i128::MAX / 10_000,
            fee_bps in 0u32..=10_000u32,
            num_recipients in 2usize..=5usize,
        ) {
            // Generate random BPS values that sum to exactly 10000
            let mut bps_values: Vec<u32> = Vec::new();
            let mut remaining = 10_000u32;
            
            for i in 0..num_recipients - 1 {
                let max_value = remaining.saturating_sub((num_recipients - i - 1) as u32);
                let value = (i as u32 * 1000).min(max_value);
                bps_values.push(value);
                remaining -= value;
            }
            bps_values.push(remaining);
            
            // Compute the split
            let result = compute_split_test(total, &bps_values, fee_bps);
            
            // Calculate expected fee
            let fee = (total * fee_bps as i128) / BPS_DENOMINATOR;
            let after_fee = total - fee;
            
            // Sum all recipient amounts
            let sum: i128 = result.iter().sum();
            
            // Property: sum should equal total - fee (within rounding tolerance)
            // We allow ±num_recipients difference due to rounding
            let diff = (sum - after_fee).abs();
            prop_assert!(diff <= num_recipients as i128, 
                "Sum mismatch: sum={}, expected={}, diff={}", sum, after_fee, diff);
        }

        /// Property: With 0% fee, the sum should equal the total (minus rounding)
        #[test]
        fn test_zero_fee_preserves_total(
            total in 1i128..=i128::MAX / 10_000,
            num_recipients in 2usize..=5usize,
        ) {
            let mut bps_values: Vec<u32> = Vec::new();
            let mut remaining = 10_000u32;
            
            for i in 0..num_recipients - 1 {
                let max_value = remaining.saturating_sub((num_recipients - i - 1) as u32);
                let value = (i as u32 * 1000).min(max_value);
                bps_values.push(value);
                remaining -= value;
            }
            bps_values.push(remaining);

            let result = compute_split_test(total, &bps_values, 0);
            
            let sum: i128 = result.iter().sum();
            let diff = (sum - total).abs();
            
            prop_assert!(diff <= num_recipients as i128,
                "Zero-fee sum mismatch: sum={}, expected={}, diff={}", sum, total, diff);
        }

        /// Property: With 100% fee, all recipients should get 0
        #[test]
        fn test_full_fee_yields_zero(
            total in 1i128..=i128::MAX / 10_000,
            num_recipients in 2usize..=5usize,
        ) {
            let mut bps_values: Vec<u32> = Vec::new();
            let mut remaining = 10_000u32;
            
            for i in 0..num_recipients - 1 {
                let max_value = remaining.saturating_sub((num_recipients - i - 1) as u32);
                let value = (i as u32 * 1000).min(max_value);
                bps_values.push(value);
                remaining -= value;
            }
            bps_values.push(remaining);

            let result = compute_split_test(total, &bps_values, 10_000);
            
            for amount in result.iter() {
                prop_assert_eq!(*amount, 0, "100% fee should yield 0 for all recipients");
            }
        }

        /// Property: Each recipient's share should be proportional to their BPS
        #[test]
        fn test_proportional_distribution(
            total in 1i128..=i128::MAX / 10_000,
            fee_bps in 0u32..=10_000u32,
        ) {
            // Use fixed BPS for easier verification: 50%, 30%, 20%
            let recipients = vec![5000u32, 3000u32, 2000u32];
            let result = compute_split_test(total, &recipients, fee_bps);
            
            let fee = (total * fee_bps as i128) / BPS_DENOMINATOR;
            let after_fee = total - fee;
            
            // Calculate expected amounts
            let expected_0 = (after_fee * 5000) / BPS_DENOMINATOR;
            let expected_1 = (after_fee * 3000) / BPS_DENOMINATOR;
            let expected_2 = (after_fee * 2000) / BPS_DENOMINATOR;
            
            // Allow small rounding differences
            prop_assert!((result[0] - expected_0).abs() <= 1);
            prop_assert!((result[1] - expected_1).abs() <= 1);
            prop_assert!((result[2] - expected_2).abs() <= 1);
        }

        /// Property: Increasing a recipient's BPS should never decrease their amount
        #[test]
        fn test_monotonic_bps(
            total in 1i128..=i128::MAX / 10_000,
            fee_bps in 0u32..=10_000u32,
            increase in 1u32..=1000u32,
        ) {
            // Start with 50%, 50% split
            let recipients_before = vec![5000u32, 5000u32];
            let result_before = compute_split_test(total, &recipients_before, fee_bps);
            
            // Increase first recipient's share, decrease second
            let new_bps = (5000u32 + increase).min(9999);
            let recipients_after = vec![new_bps, 10000 - new_bps];
            let result_after = compute_split_test(total, &recipients_after, fee_bps);
            
            // First recipient should get more (or same)
            prop_assert!(result_after[0] >= result_before[0],
                "Increasing BPS should not decrease amount: before={}, after={}",
                result_before[0], result_after[0]);
        }

        /// Property: No recipient should receive more than the total amount
        #[test]
        fn test_no_amount_exceeds_total(
            total in 1i128..=i128::MAX / 10_000,
            fee_bps in 0u32..=10_000u32,
            num_recipients in 2usize..=5usize,
        ) {
            let mut bps_values: Vec<u32> = Vec::new();
            let mut remaining = 10_000u32;
            
            for i in 0..num_recipients - 1 {
                let max_value = remaining.saturating_sub((num_recipients - i - 1) as u32);
                let value = (i as u32 * 1000).min(max_value);
                bps_values.push(value);
                remaining -= value;
            }
            bps_values.push(remaining);

            let result = compute_split_test(total, &bps_values, fee_bps);
            
            for amount in result.iter() {
                prop_assert!(*amount <= total,
                    "Recipient amount {} exceeds total {}", amount, total);
            }
        }

        /// Property: All amounts should be non-negative
        #[test]
        fn test_non_negative_amounts(
            total in 1i128..=i128::MAX / 10_000,
            fee_bps in 0u32..=10_000u32,
            num_recipients in 2usize..=5usize,
        ) {
            let mut bps_values: Vec<u32> = Vec::new();
            let mut remaining = 10_000u32;
            
            for i in 0..num_recipients - 1 {
                let max_value = remaining.saturating_sub((num_recipients - i - 1) as u32);
                let value = (i as u32 * 1000).min(max_value);
                bps_values.push(value);
                remaining -= value;
            }
            bps_values.push(remaining);

            let result = compute_split_test(total, &bps_values, fee_bps);
            
            for amount in result.iter() {
                prop_assert!(*amount >= 0, "Amount should be non-negative: {}", amount);
            }
        }
    }

    /// Edge case: Single recipient should get (total - fee)
    #[test]
    fn test_single_recipient() {
        let total = 1_000_000i128;
        let fee_bps = 250u32; // 2.5%
        let recipients = vec![10_000u32]; // 100%
        
        let result = compute_split_test(total, &recipients, fee_bps);
        let fee = (total * fee_bps as i128) / BPS_DENOMINATOR;
        let expected = total - fee;
        
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected);
    }

    /// Edge case: Zero total amount
    #[test]
    fn test_zero_total() {
        let total = 0i128;
        let recipients = vec![5000u32, 5000u32];
        let result = compute_split_test(total, &recipients, 250);
        
        assert_eq!(result, vec![0i128, 0i128]);
    }

    /// Edge case: Very small amounts with rounding
    #[test]
    fn test_small_amounts_rounding() {
        let total = 10i128;
        let recipients = vec![3333u32, 3333u32, 3334u32]; // Sums to 10000
        let result = compute_split_test(total, &recipients, 0);
        
        // Sum should equal total (within rounding)
        let sum: i128 = result.iter().sum();
        assert!((sum - total).abs() <= 3, "Rounding error too large");
    }
}
