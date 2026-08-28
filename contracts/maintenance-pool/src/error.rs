use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
#[non_exhaustive]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    PoolNotFound = 4,
    TokenMismatch = 5,
    InvalidAmount = 6,
    InsufficientBalance = 7,
    InvalidFee = 8,
    /// The deposit's inactivity window has not yet elapsed (issue #42).
    InactivityWindowNotElapsed = 9,
    /// The caller is not the deposit's original sponsor (issue #42).
    NotDepositSponsor = 10,
    /// The deposit index is out of range (issue #42).
    DepositNotFound = 11,
}
