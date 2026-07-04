use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
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
}
