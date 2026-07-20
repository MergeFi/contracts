# `refund`'s permissionless-after-deadline path: economics, griefing, and sponsor control

Focused analysis for [#30](https://github.com/MergeFi/contracts/issues/30).
`refund` (`contracts/escrow/src/lib.rs`) is the only state-changing
function in the entire system callable by a completely arbitrary,
unauthenticated caller once `env.ledger().timestamp() >= escrow.deadline`.
No equivalent exists in `milestones` or `maintenance-pool` — both
`cancel_milestone` and `withdraw` are admin-only with no timeout escape
hatch (worth noting as a real inconsistency across the three contracts'
sponsor-protection guarantees, discussed at the end).

## Does calling `refund` on someone else's behalf cost the caller anything?

Yes. Whoever submits the transaction pays that transaction's Stellar
resource fee (a small but non-zero, non-refundable cost). `refund` has
no tip/reward mechanism — the caller gets nothing back for calling it;
100% of the escrowed amount goes to `escrow.sponsor` regardless of who
submitted the transaction. So calling `refund` for a stranger is a
strictly negative-EV action for the caller: real cost, zero benefit.

## Is "anyone can call it" realistic, or does it implicitly assume `mergefi-backend` is the one calling?

Both, but for different reasons, and the honest answer requires
separating "who realistically calls this in the common case" from
"what is the fallback actually protecting against."

**In the common case**, the realistic caller is one of:

- `mergefi-backend` itself. The README says it "likely does [call
  `refund`] for UX (so sponsors don't have to trigger it manually)" —
  the backend already holds signing keys and infrastructure to submit
  transactions for `release`/`allocate`/`withdraw`, so calling the
  permissionless `refund` path costs it nothing extra to automate.
- The sponsor themselves. They have a direct economic incentive — it's
  their own money coming back — and can trigger it via the same wallet
  they used to call `fund`.

Neither of these needs `refund` to be permissionless — the backend
could just as easily be given `admin.require_auth()` for this path too
(it already has that for the pre-deadline case), and the sponsor could
self-authorize like `extend_deadline` (added in this PR) does. Random
altruistic third parties, with no relationship to the sponsor and no
reward, are **not** a realistic source of calls — the negative-EV
argument above means no rational disinterested party calls this. So in
the day-to-day case, "anyone can call it" is **not load-bearing at
all**; it's cosmetic relative to what actually happens on-chain.

**The fallback is load-bearing, but only in the case it's designed for:
backend unavailability.** If `mergefi-backend`'s operator ceases
operating, loses its signing key, or is otherwise unable or unwilling to
submit the `refund` transaction, the *sponsor* can still always call
`refund` themselves once the deadline passes — this doesn't even need
permissionlessness, since the sponsor could self-authorize. Where
permissionlessness earns its keep is one level further: if the sponsor
*also* can't or won't act (unresponsive, funds controlled by a multisig
that's hard to coordinate quickly, sponsor is an organization that's
since dissolved, etc.), a **third party who has some other reason to
care** — e.g. the maintainer who was hoping to get paid and wants the
issue reopened for a fresh bounty, a community member helping unwind a
stalled sponsorship, or a future replacement service standing in for a
defunct `mergefi-backend` — can still return the funds to the sponsor's
address with no coordination or special permission needed. That's a
genuine bankruptcy/custody-remoteness property: **no single party's
unavailability can permanently strand a sponsor's expired funds.** It's
a real guarantee, just one that matters in the tail case, not the
median case.

## Could a malicious "anyone" grief a sponsor by calling `refund` at the earliest possible moment?

No — not in the sense of causing the sponsor harm they didn't already
agree to. Two things bound this:

1. **Funds always go to the address stored in the record**
   (`escrow.sponsor`), never the caller. There is no parameter the
   caller controls that affects where money moves. So the *only*
   possible harm is timing, not redirection or theft.
2. **The deadline is the sponsor's own prior commitment.** The sponsor
   chose `deadline` themselves at `fund()` time. Refund becoming
   callable-by-anyone at that exact moment is the contract keeping the
   promise the sponsor asked it to keep ("if this isn't resolved by
   time T, give my money back") — not a third party overriding the
   sponsor's preference. Calling it right at `T` is not materially
   different from the sponsor having set an alarm and calling it
   themselves at `T`; a third party doing it a few seconds earlier than
   the sponsor got around to it isn't a new harm, it's the same outcome
   arriving slightly sooner.

The real gap isn't griefing — it's that **the sponsor has no way to
change their mind** after `fund()`. If a sponsor watches a PR get close
to merging right as the deadline approaches and would rather wait a bit
longer, the current contract gives them zero mechanism to express that
— `deadline` is write-once. That's a legitimate design gap the issue
asks to be resolved.

## Design decision: sponsor-settable extension, not an admin/backend-controlled grace period

The issue asks whether to "implement a sponsor-settable auto-extend /
grace-period flag... balancing against not undermining the
sponsor-protective guarantee." Two shapes were considered:

- **Admin/backend-controlled grace period.** Rejected. Letting the
  admin unilaterally delay `refund`'s permissionless window would
  reintroduce exactly the dependency the permissionless design exists
  to remove — a sponsor's guaranteed recovery time would no longer be
  guaranteed if the backend (the same party whose *unavailability* is
  the scenario refund protects against) could also postpone it. Worse,
  it's a strict downgrade for sponsors trying to escape an
  unresponsive/compromised backend, which is the one scenario where
  permissionlessness matters most.

- **Sponsor-controlled, monotonic-only extension — implemented as
  `extend_deadline` in this PR.** `escrow.sponsor.require_auth()`,
  callable only while `status == Funded`, and only accepted if
  `new_deadline` is strictly later than both the current stored
  deadline and the current ledger time (`Error::InvalidDeadline`
  otherwise). This directly answers the issue's question with a real
  mechanism: a sponsor who wants more runway can extend it themselves,
  at any point before the funds are paid out or refunded, including
  after the original deadline has already passed (re-closing a window
  that had opened, if nobody has claimed it yet). Because only the
  sponsor can call it and it can only push the deadline later, it can't
  be used by the admin/backend or any third party to trap funds against
  the sponsor's will, and it can't be used to retroactively undo a
  `refund` that already executed (blocked by the `Funded`-only status
  check, same as the existing early-refund admin path). This preserves
  the exact property that makes `refund`'s permissionlessness valuable
  (nobody but the sponsor controls their own timeline) while closing
  the "sponsor has no say once deadline passes" gap the issue raises.

## Cross-contract inconsistency (noted, not fixed here)

`milestones::cancel_milestone` and `maintenance-pool::withdraw` are
both strictly admin-only with no timeout/escape-hatch concept at all —
unlike escrow, a sponsor whose milestone or maintenance-pool admin goes
permanently unresponsive has **no path to recover undistributed funds**
except an off-chain intervention (e.g. a contract upgrade or manual
backend action outside this system). This is a real asymmetry across
the three contracts' sponsor-protection guarantees. It's out of scope
for this PR (adding a deadline-based recovery concept to milestones/
maintenance-pool is a modeling change, not an access-control fix — pool
deposits in particular have no natural single "deadline" the way a
single-issue escrow does), but worth a dedicated design issue if the
product wants parity. Not filed as a new issue here since it would
need product input (what would "deadline" even mean for an open-ended,
repeatedly-topped-up maintenance pool?) rather than a purely technical
decision.
