use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
#[non_exhaustive]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    MilestoneNotFound = 3,
    IssueAlreadyAllocated = 4,
    IssueNotAllocated = 5,
    IssueAlreadyReleased = 6,
    OverAllocation = 7,
    InvalidSplit = 8,
    InvalidAmount = 9,
    InvalidFee = 10,
    MilestoneClosed = 11,
    TooManySponsors = 12,
    /// Treasury cannot be set to the contract's own address (issue #39).
    InvalidTreasury = 13,
    /// A milestone with this id already exists and is not in a terminal state (issue #41).
    MilestoneAlreadyExists = 14,
    /// The milestone's deadline has not yet passed (issue #42).
    DeadlineNotPassed = 15,
    /// The issue is not allocated, so it cannot be deallocated (issue #43).
    IssueNotAllocatedForDeallocate = 16,
    /// Contract is paused and this operation is not allowed (issue #14).
    ContractPaused = 17,
    /// The contribution index is out of range for an existing milestone (issue #256).
    ContributionNotFound = 18,
    /// A recipient address equals the contract's own address — a self-payout
    /// that would strand funds in the contract with no recovery path.
    SelfPayout = 19,
    /// Checked i128 arithmetic overflowed (e.g. budget total near
    /// `i128::MAX`). Returned instead of panicking under release
    /// `overflow-checks = true` + `panic = "abort"`.
    ArithmeticOverflow = 20,
    /// `allocate` or `contribute` was called on a milestone whose deadline has
    /// already passed (#489, #568). Once past the deadline, sponsors may cancel;
    /// allowing further allocations or contributions would let late funds enter
    /// a wind-down window or let the admin pre-empt that cancel.
    DeadlinePassed = 21,
    /// The deadline supplied to `create_milestone` is in the past (issue #491).
    DeadlineInPast = 22,
    /// The stored contract version does not match the requested migration source.
    InvalidVersion = 23,
    /// `initialize` supplied a sponsor cap above `MAX_SPONSORS_LIMIT`.
    InvalidMaxSponsors = 24,
    /// The payout recipient list exceeds `MAX_RECIPIENTS`.
    TooManyRecipients = 25,
}
