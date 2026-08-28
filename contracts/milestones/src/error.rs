use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
#[non_exhaustive]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    MilestoneNotFound = 4,
    IssueAlreadyAllocated = 5,
    IssueNotAllocated = 6,
    IssueAlreadyReleased = 7,
    OverAllocation = 8,
    InvalidSplit = 9,
    InvalidAmount = 10,
    InvalidFee = 11,
    MilestoneClosed = 12,
    TooManySponsors = 13,
    /// A milestone with this id already exists and is not in a terminal state (issue #41).
    MilestoneAlreadyExists = 14,
    /// The milestone's deadline has not yet passed (issue #42).
    DeadlineNotPassed = 15,
    /// The issue is not allocated, so it cannot be deallocated (issue #43).
    IssueNotAllocatedForDeallocate = 16,
}
