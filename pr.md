## Summary

- **Instance storage TTL extension (Closes #38):** Added `env.storage().instance().extend_ttl(100_000, 500_000)` to all state-changing entrypoints across all three contracts (`escrow`, `milestones`, `maintenance-pool`). Previously, instance storage (holding Admin, Treasury, FeeBps, MaxSponsors) was never extended, risking full contract archival — unlike per-record persistent TTL which was already handled. Now every mutating call keeps the instance alive alongside individual records.

- **Identifier reuse after terminal state (Closes #41):** `escrow::fund` and `milestones::create_milestone` now allow re-funding/re-creation when the existing record is in a terminal state (Paid/Refunded for escrow, closed for milestones). Previously, once an `issue_id` or `milestone_id` reached any terminal state, the identifier was permanently retired with no recovery path. The existing record is overwritten with fresh state on reuse. Active (Funded/open) records still reject duplicates as before.

- **Milestone deadline + permissionless cancel (Closes #42):** Added a `deadline` field to `Milestone` (set at `create_milestone`) and a new `cancel_milestone_after_deadline` entrypoint that mirrors escrow's permissionless `refund` — anyone can trigger it after `deadline + GRACE_PERIOD` (14 days), but funds only go to contributors on record. For maintenance-pool, added a per-deposit `reclaim_deposit` entrypoint: sponsors can reclaim individual deposits after a 90-day inactivity window (`INACTIVITY_WINDOW`) if no `withdraw` has occurred against the pool. Both mechanisms provide non-admin-gated recovery paths for sponsors whose admin goes permanently unresponsive.

- **Deallocate mechanism (Closes #43):** Added `deallocate(milestone_id, issue_id)` to milestones — admin-only, moves an Allocated (not yet Released) issue's amount back into `remaining_budget`, removes the allocation entry, and clears its `IssueStatus`. This prevents the scenario where #5's fix (blocking `release_issue` on closed milestones) would strand allocated-but-unreleased funds permanently. A deallocated issue can be re-allocated with corrected amounts.

## Test plan

- `cargo test --workspace` — all existing tests pass (updated for new `create_milestone` signature)
- New escrow tests: `test_fund_allows_reuse_after_refund`, `test_fund_allows_reuse_after_paid`, `test_fund_still_rejects_reuse_of_funded_escrow`
