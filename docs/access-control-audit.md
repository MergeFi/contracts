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
| `refund` (before `deadline`) | Admin-only | `require_admin(&env)?.require_auth()` | unchanged | Match |
| `refund` (at/after `deadline`) | Permissionless (deliberate) | none | unchanged | Match — see [refund analysis](./refund-permissionless-analysis.md) |
| `extend_deadline` (new, this PR) | Sponsor-only, monotonic | n/a | `escrow.sponsor.require_auth()` + `new_deadline` must strictly increase | Match (new function) |
| `get_escrow` | Permissionless (view) | none | unchanged | Match |
| `get_admin` | Permissionless (view) | none | unchanged | Match |
| `get_treasury` | Permissionless (view) | none | unchanged | Match |
| `get_fee_bps` | Permissionless (view) | none | unchanged | Match |

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

## `contracts/maintenance-pool` (`mergefi-maintenance-pool`)

| Function | Intended access | Actual (before this PR) | Actual (after this PR) | Verdict |
|---|---|---|---|---|
| `initialize` | Deployer/authorized setup only (implicit) | none | `admin.require_auth()` | **Mismatch, fixed** |
| `deposit` | Sponsor-only | `sponsor.require_auth()` | unchanged | Match |
| `withdraw` | Admin-only | `require_admin(&env)?.require_auth()` | unchanged | Match |
| `get_pool` | Permissionless (view) | none | unchanged | Match |
| `get_deposit` | Permissionless (view) | none | unchanged | Match |

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

### 3. `Error::Unauthorized` is dead code in all three contracts (not a bug)

All three `error.rs` files define an `Unauthorized` variant that is
never constructed anywhere. This isn't a mismatch — Soroban's
`Address::require_auth()` traps/panics on failure rather than returning
a `Result`, so a failed auth check never reaches a point where the
contract could return `Err(Error::Unauthorized)`. Noted here only so a
future reader doesn't mistake the unused variant for a missed check.

### 4. Out of scope, cross-referenced

- `escrow::fund` has no protection against `issue_id` squatting by an
  unrelated caller — tracked in
  [#1](https://github.com/MergeFi/contracts/issues/1), not an
  access-control boundary issue (the *sponsor* access check on `fund`
  itself is correct; the gap is that anyone can supply themselves as
  `sponsor` for an `issue_id` they don't "own" in any off-chain sense).
- `escrow::fund`'s `deadline` parameter is unvalidated (can be set in
  the past) — tracked in
  [#21](https://github.com/MergeFi/contracts/issues/21).
