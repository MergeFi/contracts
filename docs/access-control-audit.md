# Access-control boundary audit

Function-by-function audit of every public entrypoint across the three
contracts, comparing the access level the documentation (README /
doc comments) claims against what the code actually enforces. Done for
[#30](https://github.com/MergeFi/contracts/issues/30).

Legend: **Actual** describes the real runtime check, not the intent.
"none" means the function performs no `require_auth()` call at all —
i.e. it is callable by anyone who can submit a transaction, with no
signature requirement on any particular address.

## `contracts/escrow` (`mergefi-escrow`)

| Function | Intended access | Actual (before this PR) | Actual (after this PR) | Verdict |
|---|---|---|---|---|
| `initialize` | Deployer/authorized setup only (implicit — not written down anywhere) | none | `admin.require_auth()` | **Mismatch, fixed** — see "`initialize` has no access control" below |
| `fund` | Sponsor-only | `sponsor.require_auth()` | unchanged | Match |
| `release` | Admin-only | `require_admin(&env)?.require_auth()` | unchanged | Match |
| `refund` (before `deadline + GRACE_PERIOD`) | Admin-only | `require_admin(&env)?.require_auth()` | unchanged | Match |
| `refund` (at/after `deadline + GRACE_PERIOD`) | Permissionless (deliberate) | none | unchanged | Match — see [refund analysis](./refund-permissionless-analysis.md). The permissionless window opens at `deadline + GRACE_PERIOD`, not at `deadline` itself; the 14-day `GRACE_PERIOD` was introduced by [#49](https://github.com/MergeFi/contracts/issues/49) to close a race where a sponsor could call permissionless `refund()` right at `deadline` to claw back funds from a contributor whose `release()` had already landed. |
| `extend_deadline` (new, this PR) | Sponsor-only, monotonic | n/a | `escrow.sponsor.require_auth()` + `new_deadline` must strictly increase | Match (new function) |
| `get_escrow` | Permissionless (view) | none | unchanged | Match |
| `get_admin` | Permissionless (view) | none | unchanged | Match |
| `get_treasury` | Permissionless (view) | none | unchanged | Match |
| `get_fee_bps` | Permissionless (view) | none | unchanged | Match |
| `contribute` | Sponsor-only | n/a | `sponsor.require_auth()` | Match (new function) |
| `keep_alive` | Permissionless (deliberate) | n/a | none | Match (new function) |
| `get_contribution` | Permissionless (view) | n/a | none | Match (new function) |
| `get_admin` | Permissionless (view) | n/a | none | Match (new function) |
| `get_treasury` | Permissionless (view) | n/a | none | Match (new function) |
| `set_admin` | Admin-only | n/a | `require_admin(...).require_auth()` | Match (new function) |
| `recover_admin` | Recovery-only (initialize-time) | n/a | `recovery.require_auth()` | Match (new function) |
| `set_treasury` | Admin-only | n/a | `require_admin(...).require_auth()` | Match (new function) |
| `get_contributions` | Permissionless (view) | n/a | none | Match (new function) |
| `set_treasury` | Admin-only | n/a | `require_admin(&env)?.require_auth()` | Match (new function) |
| `pause` | Admin-only | n/a | `require_admin(...).require_auth()`; blocks new state-changing calls while set | Match (new function) |
| `unpause` | Admin-only | n/a | `require_admin(...).require_auth()` | Match (new function) |
| `is_paused_view` | Permissionless (view) | n/a | none | Match (new function) |
| `upgrade` | Admin-only | n/a | `require_admin(...).require_auth()` | Match (new function) |
| `get_version` | Permissionless (view) | n/a | none | Match (new function) |
| `set_oracle` | Admin-only | n/a | `require_admin(...).require_auth()` + `new_oracle.require_auth()` | Match (new function) |
| `set_fee_bps` | Admin-only | n/a | `require_admin(...).require_auth()` | Match (new function) |
| `get_max_sponsors` | Permissionless (view) | n/a | none | Match (new function) |

## `contracts/milestones` (`mergefi-milestones`)

| Function | Intended access | Actual (before this PR) | Actual (after this PR) | Verdict |
|---|---|---|---|---|
| `initialize` | Deployer/authorized setup only (implicit) | none | `admin.require_auth()` | **Mismatch, fixed** |
| `create_milestone` | Sponsor-only | `sponsor.require_auth()` | unchanged | Match |
| `allocate` | Admin-only | `require_admin(&env)?.require_auth()` | unchanged | Match |
| `release_issue` | Admin-only | `require_admin(&env)?.require_auth()` | unchanged | Match — access control itself is correct; see note below on the *separate* state-machine gap tracked in #5 |
| `cancel_milestone` | Admin-only | `require_admin(&env)?.require_auth()` | unchanged | Match |
| `get_milestone` | Permissionless (view) | none | unchanged | Match |
| `get_issue_status` | Permissionless (view) | none | unchanged | Match |
| `contribute` | Sponsor-only | n/a | `sponsor.require_auth()` | Match (new function) |
| `keep_alive` | Permissionless (deliberate) | n/a | none | Match (new function) |
| `get_admin` | Permissionless (view) | n/a | none | Match (new function) |
| `get_treasury` | Permissionless (view) | n/a | none | Match (new function) |
| `set_admin` | Admin-only | n/a | `require_admin(...).require_auth()` | Match (new function) |
| `recover_admin` | Recovery-only (initialize-time) | n/a | `recovery.require_auth()` | Match (new function) |
| `set_treasury` | Admin-only | n/a | `require_admin(...).require_auth()` | Match (new function) |
| `pause` | Admin-only | n/a | `require_admin(...).require_auth()`; blocks new state-changing calls while set | Match (new function) |
| `unpause` | Admin-only | n/a | `require_admin(...).require_auth()` | Match (new function) |
| `is_paused_view` | Permissionless (view) | n/a | none | Match (new function) |
| `upgrade` | Admin-only | n/a | `require_admin(...).require_auth()` | Match (new function) |
| `get_version` | Permissionless (view) | n/a | none | Match (new function) |
| `set_oracle` | Admin-only | n/a | `require_admin(...).require_auth()` + `new_oracle.require_auth()` | Match (new function) |
| `set_fee_bps` | Admin-only | n/a | `require_admin(...).require_auth()` | Match (new function) |
| `deallocate` | Admin-only | n/a | `require_admin(...).require_auth()`; **not pause-gated** (unlike `allocate`/`release_issue`) — a pause-gating inconsistency tracked separately | Match (new function) |
| `cancel_milestone_after_deadline` | Permissionless after `deadline + GRACE_PERIOD` (deliberate) | n/a | none; rejects with `DeadlineNotPassed` before then | Match (new function) |
| `get_max_sponsors` | Permissionless (view) | n/a | none | Match (new function) |
| `get_oracle` | Permissionless (view) | n/a | none | Match (new function) |
| `get_issue_status` | Permissionless (view) | n/a | none | Match (new function) |
| `get_contribution` | Permissionless (view) | n/a | none | Match (new function) |
| `get_contributions` | Permissionless (view) | n/a | none | Match (new function) |

## `contracts/maintenance-pool` (`mergefi-maintenance-pool`)

| Function | Intended access | Actual (before this PR) | Actual (after this PR) | Verdict |
|---|---|---|---|---|
| `initialize` | Deployer/authorized setup only (implicit) | none | `admin.require_auth()` | **Mismatch, fixed** |
| `deposit` | Sponsor-only | `sponsor.require_auth()` | unchanged | Match |
| `withdraw` | Admin-only | `require_admin(&env)?.require_auth()` | unchanged | Match |
| `get_pool` | Permissionless (view) | none | unchanged | Match |
| `get_deposit` | Permissionless (view) | none | unchanged | Match |
| `keep_alive` | Permissionless (deliberate) | n/a | none | Match (new function) |
| `pause` | Admin-only | n/a | `require_admin(...).require_auth()`; blocks new state-changing calls while set | Match (new function) |
| `unpause` | Admin-only | n/a | `require_admin(...).require_auth()` | Match (new function) |
| `is_paused_view` | Permissionless (view) | n/a | none | Match (new function) |
| `upgrade` | Admin-only | n/a | `require_admin(...).require_auth()` | Match (new function) |
| `get_version` | Permissionless (view) | n/a | none | Match (new function) |
| `set_oracle` | Admin-only | n/a | `require_admin(...).require_auth()` + `new_oracle.require_auth()` | Match (new function) |
| `reclaim_deposit` | Sponsor-only | n/a | `sponsor.require_auth()`; intentionally **not** pause-gated so sponsors can always exit | Match (new function) |
| `sweep` | Admin-only | n/a | `require_admin(...).require_auth()`; intentionally **not** pause-gated | Match (new function) |
| `set_admin` | Admin-only | n/a | `require_admin(...).require_auth()` + `new_admin.require_auth()` | Match (new function) |
| `set_treasury` | Admin-only | n/a | `require_admin(...).require_auth()` | Match (new function) |
| `recover_admin` | Recovery-only (initialize-time) | n/a | `recovery.require_auth()` + `new_admin.require_auth()` | Match (new function) |
| `get_fee_bps` | Permissionless (view) | n/a | none | Match (new function) |
| `get_oracle` | Permissionless (view) | n/a | none | Match (new function) |

## Findings

### 1. `initialize` has no access control in any of the three contracts

Before this PR, `initialize(admin, treasury, fee_bps)` performed **zero**
`require_auth()` calls in all three contracts — the only guard is
`storage().instance().has(&DataKey::Admin)`, which prevents
*re*-initialization but does nothing to gate the *first* call. Any
account could call `initialize` on a freshly-deployed, not-yet-initialized
contract and name itself (or anyone) as `admin`/`treasury`.

**Fix applied in this PR:** all three `initialize` functions now call
`admin.require_auth()` before writing any state.

**What this fix does and does not solve — read carefully, this is not a
complete fix:**

- It *does* mean nobody can name a specific third-party address as
  `admin` without that address's key signing the invocation. Previously
  any string of the right type could be passed with no verification the
  named party consented to the role at all.
- It does **not** stop an attacker from front-running the legitimate
  deployer's `initialize` call by calling `initialize(attacker_addr,
  attacker_addr, 10_000)` themselves — the attacker trivially satisfies
  `require_auth()` by naming their own address, which they can of
  course sign for. This is the same class of race described for
  `escrow::fund` in
  [#1](https://github.com/MergeFi/contracts/issues/1) (issue-id
  squatting), and it is **not solvable by any in-contract signature
  check**, because the entire vulnerability is "whoever's transaction
  lands first, wins" — there is no address to check a signature
  *against* until that transaction has already executed.

  The actual fix for this class of bug is structural: use Soroban's
  native constructor (`__constructor`, supported since roughly
  soroban-sdk 21/22 — this repo is on 26.1.0) so that contract creation
  and initialization happen atomically in a single host operation, with
  no separate `initialize` transaction and therefore no window for a
  racer to land first. That is a real API/deploy-flow change (it
  touches `scripts/deploy.mjs`, the `Makefile` deploy targets, and the
  README's deploy instructions, and it's not backward compatible with
  the already-initialized testnet deployments listed in the README) —
  too large and orthogonal to bundle into an access-control audit PR.
  Filed as
  [#33](https://github.com/MergeFi/contracts/issues/33) to track the
  constructor migration separately.

  Given that, the `require_auth()` addition in this PR should be read
  as a correctness/consistency improvement (every other privileged
  `Address` parameter in these contracts is `require_auth`'d; `admin`
  in `initialize` was the one exception), **not** as a claim that
  initializer front-running is now closed.

### 2. `release_issue` / `cancel_milestone` "closed" asymmetry — not an access-control bug, cross-referenced not duplicated

`cancel_milestone` checks `milestone.closed` and rejects with
`MilestoneClosed`; `release_issue` performs no equivalent check, so an
already-allocated-but-not-yet-released issue can still be released via
`release_issue` after the milestone has been cancelled. This is real,
but it is a **state-machine / business-logic gap, not an access-control
one** — `release_issue` still correctly requires admin auth in both
cases; the bug is about *what state* the admin is allowed to act on,
not *who* is allowed to act. It's already tracked in detail in
[#5](https://github.com/MergeFi/contracts/issues/5) and is intentionally
**not** fixed in this PR to avoid two concurrent PRs racing on the same
lines of `milestones/src/lib.rs`.

### 3. `Error::Unauthorized` usage and dead code

In `contracts/escrow`, `Error::Unauthorized` is actively constructed and returned by `extend_deadline` (`contracts/escrow/src/lib.rs:475-477`) when the caller is not an existing contributor for the given issue (verified in `contracts/escrow/src/test.rs:985`).

In `contracts/milestones` and `contracts/maintenance-pool`, the `Unauthorized` variant in `error.rs` is unused/dead code by design. Soroban's `Address::require_auth()` traps/panics on signature failure rather than returning a `Result`, so a failed auth check never reaches a point where those contracts return `Err(Error::Unauthorized)`. Noted here so a future reader understands why the variant appears unused in those two contracts.

### 4. `keep_alive` functions have deliberately no-auth design

All three contracts (`escrow`, `milestones`, `maintenance-pool`) now include
`keep_alive` functions that are explicitly designed to be permissionless.
These functions only extend TTL (time-to-live) of storage records without
modifying any financial state or changing ownership. The documentation
explicitly states they are "callable by anyone and needs no authorization"
because they "can only ever keep records alive longer, never change what
they hold." This is a deliberate design choice to ensure long-running
contracts remain queryable without requiring privileged access for routine
maintenance operations.

### 5. Out of scope, cross-referenced

- `escrow::fund` has no protection against `issue_id` squatting by an
  unrelated caller — tracked in
  [#1](https://github.com/MergeFi/contracts/issues/1), not an
  access-control boundary issue (the *sponsor* access check on `fund`
  itself is correct; the gap is that anyone can supply themselves as
  `sponsor` for an `issue_id` they don't "own" in any off-chain sense).
- `escrow::fund`'s `deadline` parameter is unvalidated (can be set in
  the past) — tracked in
  [#21](https://github.com/MergeFi/contracts/issues/21).

## Recovery-address rationale

For the explicit reasoning behind the optional `recovery` address (set at
initialize and usable only with `recover_admin`), see
[docs/recovery-address-justification.md](recovery-address-justification.md#L1).
