## Summary

- **Instance storage TTL extension (Closes #38):** Added `env.storage().instance().extend_ttl(100_000, 500_000)` to all state-changing entrypoints across all three contracts (`escrow`, `milestones`, `maintenance-pool`). Previously, instance storage (holding Admin, Treasury, FeeBps, MaxSponsors) was never extended, risking full contract archival — unlike per-record persistent TTL which was already handled. Now every mutating call keeps the instance alive alongside individual records.

- **Identifier reuse after terminal state (Closes #41):** `escrow::fund` and `milestones::create_milestone` now allow re-funding/re-creation when the existing record is in a terminal state (Paid/Refunded for escrow, closed for milestones). Previously, once an `issue_id` or `milestone_id` reached any terminal state, the identifier was permanently retired with no recovery path. The existing record is overwritten with fresh state on reuse. Active (Funded/open) records still reject duplicates as before.

- **Milestone deadline + permissionless cancel (Closes #42):** Added a `deadline` field to `Milestone` (set at `create_milestone`) and a new `cancel_milestone_after_deadline` entrypoint that mirrors escrow's permissionless `refund` — anyone can trigger it after `deadline + GRACE_PERIOD` (14 days), but funds only go to contributors on record. For maintenance-pool, added a per-deposit `reclaim_deposit` entrypoint: sponsors can reclaim individual deposits after a 90-day inactivity window (`INACTIVITY_WINDOW`) if no `withdraw` has occurred against the pool. Both mechanisms provide non-admin-gated recovery paths for sponsors whose admin goes permanently unresponsive.

- **Deallocate mechanism (Closes #43):** Added `deallocate(milestone_id, issue_id)` to milestones — admin-only, moves an Allocated (not yet Released) issue's amount back into `remaining_budget`, removes the allocation entry, and clears its `IssueStatus`. This prevents the scenario where #5's fix (blocking `release_issue` on closed milestones) would strand allocated-but-unreleased funds permanently. A deallocated issue can be re-allocated with corrected amounts.

- **Merge-conflict syntax fixes:** Resolved stray merge markers in `contracts/escrow/src/lib.rs` and `contracts/milestones/src/lib.rs` that were introduced during the `max_sponsors` feature integration, causing build failures on a clean clone.

- **Panic avoidance — deposit count ceiling (Closes #45):** Replaced the unchecked `deposit_count += 1` in `contracts/maintenance-pool/src/lib.rs` with `checked_add`, returning the new `DepositCountOverflow` error variant instead of panicking at `u32::MAX`. Companion regression test `test_deposit_rejects_when_deposit_count_would_overflow` added.

- **Atomic payout revert risk documented & tested (partial mitigation, #47):** Added `MockPanicToken` contract doubles and two new tests — `test_release_all_or_nothing_revert_with_blocked_recipient` (escrow) and `test_release_issue_all_or_nothing_revert_with_blocked_recipient` (milestones) — that prove the all-or-nothing payout semantics when a frozen/unauthorized trustline blocks one recipient. The security model section in README now documents this risk and the required backend-side pre-flight trustline check.

- **Milestones budget-conservation fuzz harness (partial coverage, #54):** Added `test_milestones_invariant_fuzzing` — a 300-step deterministic property-based test running random sequences of `create_milestone`, `contribute`, `allocate`, `release_issue`, `deallocate`, and `cancel_milestone`. After every operation it asserts: (a) for open milestones `total_budget == remaining_budget + Σallocations`, and (b) every issue_id in `allocations` has a live `IssueStatus`. Confirmed the invariant holds across all generated sequences.

- **Real-network integration test design doc (progress on #50):** Added `docs/real-network-integration-testing.md` documenting the full Testnet/Mainnet integration harness: environment variables, Friendbot provisioning, WASM compilation + `soroban-cli` deployment, liveness test sequence (fund → contribute → release), and post-run verification steps.

## Test plan

- `cargo test --workspace` — all **94 tests** pass (51 escrow · 16 maintenance-pool · 27 milestones)
- New escrow tests: `test_fund_allows_reuse_after_refund`, `test_fund_allows_reuse_after_paid`, `test_fund_still_rejects_reuse_of_funded_escrow`, `test_release_all_or_nothing_revert_with_blocked_recipient`
- New maintenance-pool test: `test_deposit_rejects_when_deposit_count_would_overflow`
- New milestones tests: `test_release_issue_all_or_nothing_revert_with_blocked_recipient`, `test_milestones_invariant_fuzzing`
