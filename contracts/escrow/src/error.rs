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
}
