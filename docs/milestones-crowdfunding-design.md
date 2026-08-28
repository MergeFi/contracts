# Multi-sponsor crowdfunding for `milestones`: contribution ledger and proportional refund

Focused analysis for [#58](https://github.com/MergeFi/contracts/issues/58).
Before this change, `milestones::create_milestone` accepted exactly one
`sponsor: Address` and rejected a second call against the same
`milestone_id` outright, so a release's total budget could only ever come
from a single sponsor's wallet. This documents the design chosen to close
that gap. It is deliberately shaped like the escrow crowdfunding change
([#57](https://github.com/MergeFi/contracts/issues/57), see
`docs/escrow-crowdfunding-design.md`) so the two contracts share one
mental model and, eventually, one `Contribution` type if the
shared-crate effort (#16) lands — but the refund math here is
fundamentally different, because a milestone's budget is *partially
consumed over time* before any refund can happen.

## The structural difference from escrow

Escrow's `refund` returns the *entire* escrowed amount, so each
contributor gets back exactly the amount they put in — no proportional
math, ever. A milestone's refund, by contrast, returns only
`remaining_budget` — the unallocated leftover after some slices have been
`allocate`d (and, in future, `deallocate`d) and possibly already
`release_issue`d to real contributors. The question is: **which sponsor
gets how much of the leftover?**

The answer implemented here: each sponsor's share is a *fixed fraction of
the pool*, determined at deposit time — `contribution.amount /
total_budget` — and the leftover is sliced by those same fractions at
refund time:

```
share_i = remaining_budget * contribution_i.amount / total_budget
```

Because the contribution ledger is append-only (`create_milestone` opens
with contribution index 0, every later `contribute` appends at the next
index) and `total_budget` / `remaining_budget` are maintained
additively, each sponsor's fraction is constant for the life of the
milestone. That is what makes this correct *across* an arbitrary number
of intervening `allocate` / `release_issue` (and future `deallocate`)
calls: the refund formula never depends on *when* the leftover was
computed, only on the fixed ledger, so it cannot drift out of
proportionality no matter how the pool shrank or grew between deposit and
cancellation. The acceptance scenario pins this exactly: A puts in 700,
B puts in 300, 400 is allocated and released, and the 600 leftover comes
back as 420 / 180 (70/30 of the *unspent* remainder) — not an even split
of the 600, and not 70/30 of the nominal 1000.

### Rounding: largest-remainder, same invariant as `compute_split`

`remaining_budget * contribution.amount` rarely divides `total_budget`
evenly, so naive integer division would strand dust in the contract.
`refund_remaining_budget` uses the same largest-remainder invariant as
`compute_split`: every share is floored first, then the remaining dust
(provably strictly less than `contributor_count` units, since the
contribution amounts sum to `total_budget`) is granted one unit at a
time to the entry with the largest fractional remainder, tie-broken by
contribution index — the ledger's append order — so the outcome is fully
deterministic and independent of anything a caller controls (mirroring
the adversarial-ordering fix in `compute_split`). The full
`remaining_budget` is returned; no dust is stranded. A single-sponsor
milestone is the degenerate case: contribution 0's amount equals
`total_budget`, so its share is exactly `remaining_budget` — behavior is
unchanged from before this change.

## Contribution model: `create_milestone` creates, `contribute` appends

Identical shape to escrow (#57), for the same reasons:

- **`create_milestone` keeps its exact signature and behavior.** It is
  the create half; the original funder is recorded as contribution index
  `0`. A second `create_milestone` on the same `milestone_id` is still
  rejected, so existing single-sponsor integrations are untouched.
- **`contribute(env, milestone_id, sponsor, amount)` is the new append
  half.** It takes no `token` parameter — it reuses the token already
  recorded on the milestone, so a top-up can never silently use a
  different asset than the original funder intended, and no
  `TokenMismatch`-style error is needed.
- **`Milestone.sponsor` is retained** (the original funder, always equal
  to contribution index 0) for backward compatibility with the public
  `get_milestone` view; it is set once at creation and never changes, so
  it cannot drift from the ledger.
- **New contributions grow `remaining_budget` as well as
  `total_budget`.** Money arrives unallocated — a milestone pool is "fully
  unallocated at deposit", so a top-up adds to both the pool total and
  the unallocated remainder. (This is the one way the milestone model
  differs mechanically from escrow's `contribute`, which grows only
  `escrow.amount`.) No separate "target/goal" field was introduced, for
  the same reason as escrow: there is no on-chain concept of "fully
  funded", `allocate` just draws down whatever has accumulated.

## `MAX_SPONSORS`, storage shape

Contributions are stored as separate persistent entries,
`DataKey::Contribution(milestone_id, index)` → `Contribution { sponsor,
amount }`, one per sponsor — the same shape `escrow::Contribution` and
`maintenance-pool::Deposit` already use, rather than a growing `Vec`
inline on `Milestone`. Two reasons, unchanged from escrow:

1. **Bounded storage entries.** A `Vec` on `Milestone` would make every
   read/write of the milestone record load the entire contribution
   history, even for operations (`allocate`, `release_issue`) that never
   look at it.
2. **No unbounded growth.** `max_sponsors` (an optional `initialize`
   parameter, defaulting to the `MAX_SPONSORS` constant, 20, when omitted)
   caps `contributor_count`, so `cancel_milestone`'s (and any future
   timeout wind-down's) per-contributor loop — and, critically, the
   refund's dust distribution — is bounded by a small, per-deployment-
   tunable constant regardless of how popular a release gets. A $50,000
   release milestone expecting far more participation than a single-issue
   bounty can raise the cap at `initialize` time instead of needing a
   contract redeploy (#96).

## Authorization

`allocate` / `release_issue` / `cancel_milestone` remain admin-only,
exactly as before — how many sponsors funded the milestone is irrelevant
to who may allocate, release, or cancel it. `create_milestone` /
`contribute` require the contributor's own `require_auth()`, matching the
escrow rule that a backend key can never move a sponsor's funds *into* a
contract on their behalf.

The issue asks for an explicit decision on future *sponsor-authorized*
actions (e.g. a sponsor-triggered timeout recovery per the companion
escape-hatch issue). Decision: **reuse the escrow rule verbatim — any
current contributor may act, not unanimous and not contribution-weighted
consent.** The reasoning transfers directly: a sponsor-triggered recovery
only ever returns each contributor's own proportional share to them; it
cannot redirect anyone's money or change anyone's fraction, so a single
contributor acting unilaterally needs no weighted-vote machinery, and the
admin's independent cancel path remains the escape hatch if that action
was unwarranted. The per-contributor loop this requires is exactly the
one `cancel_milestone` already runs.

## Sequencing decision (relative to the companion issues)

This PR lands **before** both the "no deallocate/reallocate" issue and
the "no timeout escape hatch" issue, deliberately:

- The proportional-refund logic is extracted into one private helper,
  `refund_remaining_budget(env, milestone_id, milestone)`. The timeout
  escape-hatch issue can call that same helper from its wind-down path
  with zero retrofitting — this is the "implement multi-sponsor first"
  scenario the issue predicted would be simpler, and it is.
- The deallocate/reallocate issue only ever *changes*
  `remaining_budget` (growing it back after a release is unwound). It
  does not touch the contribution ledger or anyone's fixed fraction, so
  the proportional refund formula stays correct through deallocate
  cycles with no changes to `refund_remaining_budget` itself — this is
  the property the "across allocate/deallocate cycles" acceptance
  criterion in #58 is about, and it holds by construction here.
- Landing multi-sponsor first also means neither companion issue ever
  has to decide "which single sponsor gets the refund" — that question is
  already answered (proportionally, to all of them) before they start.

The one interaction to keep in mind when the escape-hatch issue lands:
its permissionless/timeout path must apply the same `MAX_SPONSORS`-bounded
loop and the same largest-remainder dust rule as `cancel_milestone`, and
both paths must set `remaining_budget = 0` / `closed = true` in the same
way, so a milestone can never be wound down twice. If that issue instead
lands first, the proportional accounting designed here is the contract
its designs will have to retrofit against.
