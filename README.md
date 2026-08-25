# MergeFi Contracts

**Where Open Source Meets Finance.**

MergeFi lets sponsors fund open-source work, maintainers turn GitHub issues
into paid bounties, and contributors get paid automatically when their work
is merged. GitHub stays the system of record for *code* — who opened what,
who merged what, what got approved. This repository is the *financial
layer*: a set of Soroban smart contracts on the Stellar network that hold
sponsor funds in escrow and release them according to rules that a
trusted, off-chain oracle (the sibling `mergefi-backend` service) reports.

Flow, end to end:

1. A sponsor funds a GitHub issue (or a milestone, or a repo's ongoing
   maintenance pool) by depositing a Stellar token into one of these
   contracts.
2. A maintainer marks the issue as bounty-eligible and a contributor does
   the work, exactly as they would on any other GitHub project.
3. `mergefi-backend` watches GitHub webhooks. When it sees the PR
   referencing the issue get merged/accepted, it calls `release` (or
   `release_issue`, or `withdraw`) on the relevant contract, authenticated
   as the contract's configured admin/oracle address.
4. The contract pays the contributor(s) — split across a team if the
   bounty had multiple collaborators — deducts a small protocol fee to the
   treasury, and marks the escrow as paid. Double-payment and
   already-refunded states are rejected at the contract level, so the
   worst the backend can do is retry a call safely.
5. If an issue is cancelled or nobody finishes the work before its
   deadline, the sponsor (or, after expiry, anyone) can trigger a refund.

GitHub remains the source of truth for *whether work happened*. These
contracts are deliberately dumb about that — they only know what the
oracle tells them — and focus entirely on holding and moving money
correctly.

## Why three contracts instead of one

The spec allows team-splits/milestones to be either separate contracts or
modules in one. This repo ships them as **three independent contract
crates** — `mergefi-escrow`, `mergefi-milestones`, `mergefi-maintenance-pool`
— reasoning:

- **Different lifecycles.** An escrow is single-issue, single-payout,
  bounded by a deadline. A milestone is a lump sum sliced across many
  issues in a release, closed once. A maintenance pool is open-ended and
  repeatedly topped up — it never "finishes." Cramming all three into one
  contract's storage model would mean one bloated `DataKey` enum and a lot
  of variants that don't apply to most calls.
- **Independent upgrade/audit surface.** If a bug is found in milestone
  allocation logic, you can fix and redeploy that contract without
  touching escrow funds that are mid-flight.
- **Team-splits are not a separate contract.** They're a parameter shape
  (`Vec<(Address, u32 basis_points)>`) accepted by `release` /
  `release_issue` in both the escrow and milestone contracts. A single
  bounty and a team bounty are the same code path; the only difference is
  how many recipients are in the vector.

The tradeoff: the basis-point split math and fee-deduction logic
(`compute_split`) is duplicated between `mergefi-escrow` and
`mergefi-milestones` rather than shared via a common library crate. For a
codebase this size the duplication is small and readable; the natural
next step if it grows is to extract a `mergefi-common` crate with shared
types/helpers, imported as a normal (non-contract) Rust dependency by each
contract crate. Noted under Roadmap.

### Known limitation: no cross-contract issue_id deduplication

Because the three contracts are fully independent — no shared storage,
no cross-contract calls, not even a common library crate today — **the
same GitHub `issue_id` can be funded simultaneously in `escrow` (via
`fund`) and allocated in `milestones` (via `allocate`), with neither
contract aware the other has claimed it.** Both can independently reach
`release`/`release_issue` and pay out in full for what is, off-chain, a
single contribution.

This is a deliberate architectural consequence of the independence
tradeoff above. The contracts provide **no on-chain defense** against
cross-contract double-funding of the same `issue_id`. Prevention is
delegated entirely to the off-chain `mergefi-backend` service, which is
expected to enforce uniqueness before calling `fund` or `allocate`. If
the backend is ever wrong, out of sync, or bypassed, the contracts will
not reject the duplicate.

This limitation is accepted as the cost of keeping contracts independently
upgradeable and audit-scoped. If the tradeoff becomes unacceptable, the
natural remediation would be introducing a fourth, minimal registry
contract (or a shared `mergefi-common` crate with a common `DataKey`
convention and a designated source-of-truth instance) — but that would
reintroduce the cross-contract coupling this design explicitly avoids.

### Split rounding and dust

Team payouts use integer token amounts, so `distributable * bps / 10000`
can leave rounding dust. Earlier versions assigned all accumulated dust to
the final recipient in the caller-supplied vector. That avoided stranded
funds, but made recipient order economically relevant.

`compute_split` now uses a largest-remainder allocation in both escrow and
milestone releases:

- each recipient first receives `floor(distributable * bps / 10000)`;
- the remaining dust is always less than `recip