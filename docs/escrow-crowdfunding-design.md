# Multi-sponsor crowdfunding for `escrow`: contribution model, refund, and `extend_deadline`

Focused analysis for [#57](https://github.com/MergeFi/contracts/issues/57).
Before this change, `escrow::fund` accepted exactly one `sponsor: Address`
and rejected a second call against the same `issue_id` outright
(`AlreadyFunded`), so there was no on-chain way for more than one sponsor
to co-fund the same issue. This documents the design chosen to close that
gap, and why the alternatives considered were rejected.

## Contribution model: `fund` creates, `contribute` appends

Two shapes were considered for letting more than one sponsor put money
into the same `issue_id`:

- **Overload `fund` to silently branch on whether the escrow already
  exists** (create it on the first call, top it up on subsequent calls).
  Rejected: it would either have to drop the existing `AlreadyFunded`
  guard entirely (a behavior change for every existing single-sponsor
  integration that relies on a second `fund` call being rejected), or
  keep some other implicit signal to distinguish "first funder" from
  "additional funder" inside one function — more surface area for a
  caller to get wrong (e.g. accidentally omitting `deadline` on a
  top-up call and having it silently ignored) for no real benefit over
  just having two functions.
- **Two functions: `fund` (create) and `contribute` (append) —
  implemented here.** `fund` keeps its exact existing behavior and
  error semantics, including `AlreadyFunded` on a second call for the
  same `issue_id`. `contribute(env, issue_id, sponsor, amount)` is the
  new entrypoint every sponsor after the first uses; it takes no
  `token`/`deadline` params at all — it reuses whatever's already
  recorded on the escrow, so there's no possibility of a top-up
  silently using a different token or deadline than the original
  funder intended, and no new `TokenMismatch`-style error needed the
  way `maintenance-pool::deposit` requires for its own multi-sponsor
  case.

An optional `target: Option<i128>` was introduced on `fund()`, stored on
`Escrow` and read-only thereafter. It is informational only: it does not
block `contribute` from pushing `amount` past it, and it does not change
`release`/`refund` behavior — `escrow.amount` is still simply the running
sum of every accepted contribution (starting with the `fund` call, i.e. the
original sponsor is contribution index `0`). `release` pays out whatever has
accumulated, same as before; `target` merely gives sponsor-facing UI an
on-chain "raised X of target Y" number instead of one tracked off-chain that
can drift from on-chain truth. `None` is the default for callers that don't
set a goal.

## Refund: exact reimbursement, not proportional splitting

Each contribution is stored as its own `Contribution { sponsor, amount }`
record (`DataKey::Contribution(issue_id, index)`), a separate persistent
entry per contributor — the same shape `maintenance-pool::Deposit` already
uses, rather than one `Vec<(Address, i128)>` field inline on `Escrow`.
Two reasons:

1. **Bounded storage entries.** A single growing `Vec` on `Escrow` means
   every read/write of the escrow record has to load and re-serialize the
   entire contribution history, even for operations (like `release`) that
   don't need it at all. Per-index entries keep `Escrow` itself small and
   let `refund`/`extend_deadline` read only what they need.
2. **No unbounded growth.** `MAX_SPONSORS` (20) caps `contributor_count`,
   so `refund`'s and `extend_deadline`'s per-contributor loops are bounded
   by a small constant regardless of how popular a bounty gets — directly
   addressing the same resource concern #8/#9 raise for `recipients`/
   `allocations` elsewhere in this codebase. Twenty is generous enough for
   any realistic crowdfunding scenario for a single GitHub issue while
   keeping the worst-case loop (and its Stellar resource cost) small and
   predictable.

Because each contribution is recorded as an *exact* amount rather than a
percentage, `refund` needs no proportional-split math at all: it iterates
`0..contributor_count`, and pays each `Contribution.amount` back to that
same `Contribution.sponsor`, verbatim. There's no rounding/dust question
the way `compute_split`'s basis-point payouts have — the sum of what goes
back out is definitionally exactly the sum of what came in, split exactly
along the lines it arrived in.

## `extend_deadline`: any current contributor, not unanimous or weighted consent

Before this change, `extend_deadline` was gated by
`escrow.sponsor.require_auth()` — trivial with exactly one possible
sponsor. With potentially many contributors, three shapes were considered:

- **Unanimous consent** (every contributor must co-sign). Rejected: adds
  real coordination cost (gathering N on-chain signatures in one
  transaction, or some multi-step approval flow that doesn't exist yet)
  for a change that's Pareto-improving for the group in the common case
  — see below.
- **Contribution-weighted consent** (e.g. majority-by-amount). Rejected:
  meaningfully more state and logic (weighted vote tallying, a threshold
  constant to pick and justify) for a decision that doesn't obviously
  need it — extending the deadline doesn't redistribute anyone's money
  or change anyone's share, so weighting by contribution size doesn't
  protect against anything a simpler rule doesn't already cover.
- **Any current contributor may extend — implemented here.** The new
  `caller: Address` parameter is checked against every recorded
  `Contribution.sponsor` for that `issue_id`; if `caller` matches any of
  them (and `caller.require_auth()` succeeds, so nobody can claim to be a
  contributor they aren't), the extension is allowed. Reasoning:
  extending only ever *delays* the point at which `refund`'s
  permissionless path opens — it can never shorten it, redirect funds, or
  change anyone's payout — and every contributor already staked into this
  escrow overwhelmingly prefers "give the work more time to land and
  trigger `release`" over "an earlier refund," since that's the entire
  reason they contributed in the first place. A single contributor acting
  unilaterally to give the group's shared bounty more time to succeed
  isn't a scenario that needs the other contributors' explicit sign-off
  the way, say, redirecting funds would. The admin's independent
  early-refund path (`refund` before `deadline`, admin-only) remains
  available as an escape hatch if an extension ever turns out to have
  been unwarranted.

This is a straightforward generalization of the existing single-sponsor
rule ("the sponsor can extend") to "any of the (now possibly several)
sponsors can extend," rather than a new, more restrictive mechanism.
