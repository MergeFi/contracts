use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
#[non_exhaustive]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    PoolNotFound = 3,
    TokenMismatch = 4,
    InvalidAmount = 5,
    InsufficientBalance = 6,
    InvalidFee = 7,
    /// Treasury cannot be set to the contract's own address (issue #39).
    InvalidTreasury = 8,
    /// The deposit's inactivity window has not yet elapsed (issue #42).
    InactivityWindowNotElapsed = 9,
    /// The caller is not the deposit's original sponsor (issue #42).
    NotDepositSponsor = 10,
    /// The deposit index is out of range (issue #42).
    DepositNotFound = 11,
    /// Contract is paused and this operation is not allowed (issue #14).
    ContractPaused = 12,
    /// The deposit count has reached its maximum limit (issue #45).
    DepositCountOverflow = 13,
    /// The pool already holds `MAX_DEPOSITS` deposits (issue #94).
    TooManyDeposits = 14,
    /// The deposit has already been reclaimed; a second reclaim is rejected
    /// to prevent the same index from draining other sponsors' funds (#479).
    AlreadyReclaimed = 15,
    /// Checked i128 arithmetic overflowed (e.g. `balance` / monotonic
    /// `total_deposited` near `i128::MAX`). Returned instead of panicking
    /// under release `overflow-checks = true` + `panic = "abort"`.
    ArithmeticOverflow = 16,
    /// Recipient basis-point weights do not sum to 10 000 (#484).
    InvalidSplit = 17,
    /// A recipient address equals the contract address — self-payout (#484).
    SelfPayout = 18,
    /// The stored contract version does not match the requested migration source.
    InvalidVersion = 19,
    /// The payout recipient list exceeds `MAX_RECIPIENTS`.
    TooManyRecipients = 20,
    /// Token address is not on the admin-managed allowlist (issue #790).
    TokenNotAllowed = 21,
}
