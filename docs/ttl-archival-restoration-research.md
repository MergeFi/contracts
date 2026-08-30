# Soroban TTL, archival, and restoration: research and per-contract analysis

Research and fix analysis for [#11](https://github.com/MergeFi/contracts/issues/11).
All three contracts call `extend_ttl`/`extend_instance_ttl` with the same
hardcoded `(100_000, 500_000)` threshold/extend-to pair, regardless of the
entry's actual expected lifetime. This doc covers (1) what Soroban's TTL and
archival model actually does, since it's a Stellar-specific mechanism with
no direct EVM analog, (2) what real-world time window that hardcoded pair
buys per contract, and (3) how that compares to each contract's stated
lifecycle.

## How Soroban TTL/archival/restoration actually works

Every Soroban contract-data ledger entry — `persistent` and `temporary`
storage entries, plus a contract's own `instance` entry — carries a
`liveUntilLedgerSeq`: the ledger sequence number after which the entry is no
longer considered live. This is the mechanism behind
[CAP-0046](https://stellar.org/protocol/cap-46) ("state archival"): unlike
Ethereum, where storage is unconditionally permanent once written (and rent
is paid indirectly via gas at write time), Soroban entries have a bounded
lifetime and must be actively kept alive or they leave the *live* ledger
state entirely.

- **What "archival" means concretely.** Once the current ledger sequence
  passes an entry's `liveUntilLedgerSeq`, the entry is evicted from the
  live BucketList — the data set validators actively maintain and that a
  transaction's footprint is checked against. It isn't deleted outright;
  it moves into a colder, non-live archival bucket, but as far as an
  ordinary contract invocation is concerned it's gone: any transaction
  whose footprint references that ledger key fails at validation/apply
  time unless the key has first been restored.
- **What happens calling into an archived entry.** A transaction's
  footprint (its declared read/write set) is checked against the live
  ledger state before execution. If any key in the footprint is archived,
  the transaction is rejected — it never reaches contract logic to
  observe a graceful "not found" the way a `None` from `.get()` would
  read in application code. Concretely, this surfaces as the transaction
  failing at the `InvokeHostFunction` operation with a "entry archived" /
  bad footprint condition, not a contract-level error variant this
  codebase's `Error` enum could ever express or catch.
- **Restoring an archived entry.** The XDR transaction model exposes a
  dedicated `RestoreFootprintOp` operation (`stellar-cli`'s `restore`
  subcommand, or the equivalent `sorobanServer.restoreFootprint`-style
  call in the various client SDKs). Its footprint declares exactly the
  archived ledger key(s) to bring back, and it pays a restoration fee
  (see below) proportional to entry size. Once applied, the entry is
  live again in the current ledger, at a *freshly re-assigned*
  `liveUntilLedgerSeq` set to the network's own minimum for that entry's
  durability (`min_persistent_entry_ttl` — see below) — not the value it
  had before archival, and with no memory of how far in the future a
  contract had once tried to extend it. A subsequent normal
  transaction can then read/write it as usual (in the same transaction as
  the restore, or a separate one, since the two are independent
  operations that can be combined in a single transaction envelope).
- **Is restoration permissionless?** Yes. `RestoreFootprintOp` requires
  only the source account's own signature to pay the fee; it does not
  require any authorization from whichever address originally wrote the
  entry, nor from this codebase's `admin`. Any account holding enough
  XLM to pay the restoration fee can restore any archived contract-data
  key it knows the identity of. This matters directly for issue #11's
  "does restoration require the original writer" question: it does not
  — anyone (a sponsor, a maintainer, an unrelated third party, or an
  automated `mergefi-backend` job) can restore a stuck escrow/pool/
  milestone once they notice it archived, so "genuinely stuck forever"
  requires *nobody* ever noticing and restoring it, not a structural
  inability to do so.
- **Eviction/rent-bump cost model.** `extend_ttl`'s resource fee at write
  time is proportional to the entry's size in bytes times the number of
  ledgers the TTL is extended by — a larger record extended further into
  the future costs more, by design (this is Soroban's "rent" mechanism:
  you pay up front for however long you want the entry to survive without
  further action). Restoration after archival is charged similarly — a
  "restore" resource fee proportional to entry size, functionally similar
  in shape to the fee an `extend_ttl` call for the same entry would have
  cost, plus the archived entry's fixed operation base fee. The **real**
  cost of archival is not primarily monetary (a restore is cheap in
  absolute Lumens terms even for these contracts' modestly-sized records)
  — it's *availability*: every legitimate contract call against that
  record fails until someone specifically issues the restore, so the
  practical cost is however long it takes for a sponsor, maintainer, or
  `mergefi-backend` to notice and act, during which the funds are
  inaccessible.
- **Current network parameters** (soroban-sdk 26.1.0's own `testutils`
  defaults, which mirror what the SDK ships as "realistic" — verify
  against live network state via `stellar network` / a current Horizon
  ledger-config query before relying on exact figures, since these are
  configurable network parameters, not protocol constants, and the SDK's
  own docs note they "could drift"):
  - Ledger close time: ~5 seconds (`APPROX_SECONDS_PER_LEDGER` in
    `contracts/common/src/lib.rs`). This has trended down over Stellar's
    history and is explicitly *not* a fixed protocol guarantee — treat it
    as an approximation to re-derive periodically, not a constant to
    hardcode forever.
  - `min_persistent_entry_ttl`: 4096 ledgers (~5.7 hours at 5s/ledger) —
    the TTL a freshly-restored (or freshly-created) persistent entry
    starts at if nothing explicitly extends it further.
  - `max_entry_ttl`: 6,312,000 ledgers (~365.3 days at 5s/ledger) — the
    hard ceiling on how far into the future *any single* `extend_ttl`
    call can push an entry's `liveUntilLedgerSeq`, exposed to contract
    code as `env.ledger().max_live_until_ledger()`. No single call, no
    matter what threshold/extend-to it passes, can ever push an entry's
    survivability past this ceiling relative to the current ledger.

The practical upshot for this codebase: a persistent entry's TTL is a
resource that decays every ledger and must be topped up by *some*
transaction before it hits zero, and the only two levers available are
(a) how far each top-up pushes the TTL out, and (b) how often a top-up
happens at all. Issue #11 is fundamentally about lever (a) being tuned
to a fixed ~29 days regardless of a record's actual expected idle period.

### A testutils caveat that shaped this fix's test design

`soroban_sdk::Env::default()` (used throughout this repo's unit tests) runs
its simulated storage host in **recording footprint mode** — the mode
designed to auto-discover a transaction's footprint during simulation, not
to faithfully reproduce a real network's enforcement of archival. Concretely,
`get_with_live_until_ledger` (the internal read path every storage
access — including this SDK's own `get_ttl()` testutils helper — goes
through) silently *revives* an already-expired persistent entry to
`min_persistent_entry_ttl` the moment anything reads it, rather than
failing the read. This means unit tests built on `Env::default()` cannot
reproduce the real network's "archived entry hard-fails until restored"
behavior directly — any test that touches an expired entry implicitly
"restores" it for free, which a real network transaction never does.

The tests added for this fix work within that constraint deliberately: they
prove periodic `keep_alive` keeps a record's TTL *healthy* across ledger-
sequence jumps that would exceed the old flat bump, and contrast that
against records left untouched, whose TTL collapses all the way down to the
network's bare `min_persistent_entry_ttl` floor over the same idle gap —
the same floor a real restored entry would start from. See the test
comments in each contract's `test.rs` (search for "recording" footprint
mode) for the precise reasoning at each assertion.

## Quantified time-window analysis

`extend_ttl(key, 100_000, 500_000)` means: "once fewer than 100,000 ledgers
remain before this entry's TTL expires, bump it back out to 500,000 ledgers
from now." At ~5 seconds/ledger:

| Value | Ledgers | Real time |
|---|---|---|
| Threshold (100,000) | 100,000 | 500,000s ≈ **5.8 days** |
| Extend-to (500,000) | 500,000 | 2,500,000s ≈ **28.9 days** |
| Network max (`max_entry_ttl`, 6,312,000) | 6,312,000 | 31,560,000s ≈ **365.3 days** |

So the flat bump buys **~29 days** of survival from whenever it last fired,
and only fires again once the remaining TTL drops under ~5.8 days — as long
as *some* write happens at least once every ~29 days, the entry never
actually gets close to expiring. The gap is exactly the case issue #11
names: an entry that goes longer than ~29 days with zero writes.

- **`escrow`.** The contract's own doc comment calls the flat bump
  "conservative defaults suitable for a multi-month bounty lifecycle," but
  ~29 days is not multi-month — it's about four weeks. A bounty funded
  with a `deadline` several months out (a realistic and explicitly
  supported case — `fund`/`contribute` take an arbitrary `u64` deadline)
  gets no automatic benefit from that far-future deadline: `fund` and
  `contribute` still only apply the flat ~29-day bump, and nothing else
  touches the record until `release`/`refund`/`extend_deadline`/
  `keep_alive` is called. If nobody calls the latter two and the escrow
  sits unresolved (a slow-moving bounty, a disputed issue, a sponsor who
  isn't actively managing it), it can archive well before its own
  `deadline` timestamp is ever reached — the record most needs to survive
  to precisely the moment it becomes least likely to have received a
  recent write. `extend_deadline` and `keep_alive` already scale toward
  the stored `deadline` (fixed for MergeFi/contracts#56, prior to this
  issue); this fix additionally makes sure `keep_alive` also carries the
  contract's own instance storage along, closing the last remaining gap
  in that path (see below).
- **`milestones`.** Same flat-bump math, and the issue's own framing
  ("milestones can plausibly span longer release cycles too") applies
  directly — `Milestone` already stores a `deadline` exactly like escrow
  does, but unlike escrow, neither `create_milestone`/`contribute` nor
  `keep_alive` had ever scaled toward it; `keep_alive`'s own doc comment
  incorrectly claimed "milestones have no natural deadline timestamp."
  This fix corrects that: `keep_alive` now scales the same way escrow's
  does, toward `milestone.deadline + GRACE_PERIOD`, and also refreshes
  instance storage.
- **`maintenance-pool`.** This is the sharpest mismatch: the contract is
  explicitly designed to be open-ended/recurring ("it never finishes" per
  the README), so there is no deadline to scale toward at all — a flat
  ~29-day bump is incompatible with *any* fixed constant, however large,
  since the lifecycle has no upper bound by design. The only structural
  fix is decoupling "how long a single top-up buys" from "how often a
  top-up must happen," which is exactly what `keep_alive` (already
  present, permissionless, requiring no deposit/withdrawal) is for. This
  fix changes what a single `keep_alive` call actually buys: instead of
  the same flat ~29-day bump every other call gets, it now requests the
  maximum a single `extend_ttl` call can grant — the network's own
  `max_live_until_ledger()` ceiling, ~365 days — so one permissionless
  ping roughly once a year is enough to keep a fully quiet pool (and its
  entire deposit history, and the contract's own instance storage) alive
  indefinitely with zero deposit/withdraw activity.

## What this fix does and does not guarantee

None of the above eliminates the need for *some* transaction to happen
periodically — Soroban has no on-chain scheduler, so nothing can make an
entry survive forever with truly zero interaction ever again. What this fix
does is (a) make each entry's survival window actually match its contract's
real lifecycle expectations rather than a one-size-fits-all ~29 days, and
(b) make sure the *tool* for bridging longer gaps (`keep_alive`, already
permissionless and already deployed for all three contracts) actually
carries every dependent piece of state — parent record, sub-records, and
instance storage — rather than leaving instance storage on the old flat
schedule while records got the new one. Operationally, closing the
remaining gap end-to-end still requires *something* — a sponsor, a
maintainer, or (most realistically) an automated `mergefi-backend` cron
job — to actually call `keep_alive` at an interval comfortably inside each
contract's now-correct survival window; that operational automation is
outside this repo's contract code and is called out here rather than
silently assumed.
