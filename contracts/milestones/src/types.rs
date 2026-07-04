use soroban_sdk::{contracttype, Address, Map};

/// A milestone pools a sponsor's lump-sum budget across several issues that
/// belong to the same release. Each issue is allocated a slice of the
/// budget up front; as issues resolve, their allocation is paid out and
/// deducted from `remaining_budget`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Milestone {
    pub sponsor: Address,
    pub token: Address,
    pub total_budget: i128,
    pub remaining_budget: i128,
    pub created_at: u64,
    pub closed: bool,
    /// issue_id -> allocated amount (0 once released and removed from the
    /// "open" set is not necessary; we track release via `IssueStatus`).
    pub allocations: Map<u64, i128>,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IssueStatus {
    Allocated,
    Released,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Treasury,
    FeeBps,
    Milestone(u64),
    IssueStatus(u64, u64), // (milestone_id, issue_id)
}
