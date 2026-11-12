use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
#[non_exhaustive]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    EscrowNotFound = 4,
    AlreadyFunded = 5,
    AlreadyPaid = 6,
    AlreadyRefunded = 7,
    InvalidSplit = 8,
    InvalidAmount = 9,
    NotExpired = 10,
    InsufficientBalance = 11,
    InvalidFee = 12,
    InvalidDeadline = 13,
    TooManySponsors = 14,
    ContributionNotFound = 15,
    /// `fund()`'s optional `target` was `Some(n)` with `n <= 0` (issue #144).
    InvalidTarget = 16,
    /// Treasury cannot be set to the contract's own address (issue #39).
    InvalidTreasury = 17,
    /// Contract is paused and this operation is not allowed (issue #14).
    ContractPaused = 19,
    /// A recipient address equals the contract's own address — a self-payout
    /// that would strand funds in the contract with no recovery path.
    SelfPayout = 20,
    /// Checked i128 arithmetic overflowed (e.g. contribution total near
    /// `i128::MAX`). Returned instead of panicking under release
    /// `overflow-checks = true` + `panic = "abort"`.
    ArithmeticOverflow = 21,
    /// The stored contract version does not match the requested migration source.
    InvalidVersion = 22,
    /// `initialize` supplied a sponsor cap above `MAX_SPONSORS_LIMIT`.
    InvalidMaxSponsors = 23,
    /// The payout recipient list exceeds `MAX_RECIPIENTS`.
    TooManyRecipients = 24,
    /// Token address is not on the admin-managed allowlist (issue #790).
    TokenNotAllowed = 25,
}
