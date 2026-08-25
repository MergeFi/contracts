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

### Split rounding and dust

Team payouts use integer token amounts, so `distributable * bps / 10000`
can leave rounding dust. Earlier versions assigned all accumulated dust to
the final recipient in the caller-supplied vector. That avoided stranded
funds, but made recipient order economically relevant.

`compute_split` now uses a largest-remainder allocation in both escrow and
milestone releases:

- each recipient first receives `floor(distributable * bps / 10000)`;
- the remaining dust is always less than `recip

## Prerequisites

- **Rust toolchain**: This project pins `rustc 1.95.0` via `rust-toolchain.toml`.
  Run `rustup install` (or simply `cargo build` — `rustup` will auto-install
  the pinned toolchain) to ensure you're using the exact same compiler
  version used for the deployed contracts.
- **WASM target**: `wasm32v1-none` (installed automatically via the
  `rust-toolchain.toml` `components`/`targets` fields).
- **Stellar CLI** (optional, for deployment/optimization):
  `cargo install --locked stellar-cli`

> ⚠️ **Do not run `cargo update` before building for verification.**
> The `Cargo.lock` file is checked into this repository and locks
> `soroban-sdk` to exactly `26.1.0` (enforced by the `=26.1.0` requirement
> in `Cargo.toml`). Running `cargo update` would defeat the pin and could
> produce different WASM bytecode.

## Reproducible Build Verification

The three contracts deployed to Stellar testnet can be independently
verified against this repository's source. The following recipe produces
byte-identical WASM to what is deployed.

### Verified Build Configuration

| Parameter | Value |
|-----------|-------|
| Git commit | `<FILL_IN_COMMIT_SHA_AFTER_VERIFICATION>` |
| Rust toolchain | `1.95.0` (pinned via `rust-toolchain.toml`) |
| `soroban-sdk` version | `26.1.0` (locked in `Cargo.lock`, exact pin in `Cargo.toml`) |
| Build profile | `release` (see `Cargo.toml` `[profile.release]`) |
| Optimization | `stellar contract optimize` (if used; see below) |

### Build Recipe

```bash
# 1. Clone and check out the exact commit to verify
# git clone https://github.com/MergeFi/contracts.git
# cd contracts
# git checkout <COMMIT_SHA>

# 2. Ensure the pinned toolchain is installed (rustup auto-installs on first cargo invocation)
rustup install

# 3. Build all three contracts in release mode — DO NOT run `cargo update` first
cargo build --target wasm32v1-none --release --workspace

# 4. (Optional but recommended) Optimize with stellar-cli — this is deterministic
#    and produces the same output as the deployed artifacts if the same stellar-cli version is used.
#    The deployed contracts were optimized with `stellar-cli 26.1.0`.
for c in escrow milestones maintenance-pool; do
  stellar contract optimize --wasm target/wasm32v1-none/release/mergefi-${c}.wasm
  mv target/wasm32v1-none/release/mergefi-${c}.optimized.wasm target/wasm32v1-none/release/mergefi-${c}.wasm
 done
```

### Expected WASM Hashes (SHA-256)

After running the recipe above against the verified commit, the resulting
WASM files should have the following SHA-256 hashes. Compare these against
the hashes of the bytecode deployed at the contract IDs below.

| Contract | Contract ID (Testnet) | WASM SHA-256 (after `stellar contract optimize`) |
|---|---|---|
| `mergefi-escrow` | `CAY77D2SFDVQYONSPYHOEWARE3UIWQDYHWWI2WXNPFBLBKR2Q4GEWXFB` | `<FILL_IN_AFTER_VERIFICATION>` |
| `mergefi-milestones` | `CBBRLSL6TM6XCNP2XBVT4GFHJ3NNPFKI2BCZQJ4U3TI7GV7DO2F2HG6F` | `<FILL_IN_AFTER_VERIFICATION>` |
| `mergefi-maintenance-pool` | `CD46U7WTEM2I77TXQI2VIBRQXOHEFEYYR2XFA7OVGTXX5M2F7Z3ZQOX2` | `<FILL_IN_AFTER_VERIFICATION>` |

> **Note**: The hashes above correspond to the *optimized* WASM (step 4 in the recipe).
> If you skip the optimization step, the hashes will differ. The deployed
> contracts were optimized with `stellar-cli 26.1.0`.

### Verifying Deployed Bytecode

To fetch the deployed WASM and compute its hash for comparison:

```bash
# Requires stellar-cli and a testnet RPC endpoint
stellar contract fetch --id CAY77D2SFDVQYONSPYHOEWARE3UIWQDYHWWI2WXNPFBLBKR2Q4GEWXFB --rpc-url https://soroban-testnet.stellar.org --network-passphrase "Test SDF Network ; September 2015" --output deployed-escrow.wasm
sha256sum deployed-escrow.wasm
# Repeat for the other two contract IDs
```

If the hashes match, the deployed bytecode corresponds exactly to the
source at the verified commit, built with the pinned toolchain and
locked dependencies.

## Decision Record: `soroban-sdk` Version Pinning

**Decision**: The `soroban-sdk` dependency in `Cargo.toml` is pinned to an
exact version (`=26.1.0`) rather than a caret range (`^26.1.0`).

**Rationale**: Even with a committed `Cargo.lock`, an exact version pin in
`Cargo.toml` provides defense-in-depth:

- It prevents accidental `cargo update` from changing the resolved version
  (Cargo respects the `=` requirement even if `Cargo.lock` is missing or
  ignored).
- It makes the intent explicit in the manifest — readers don't need to
  cross-reference `Cargo.lock` to know which version is required.
- It eliminates a class of supply-chain attacks where a malicious actor
  publishes a new `soroban-sdk` patch version and a compromised CI
  environment runs `cargo update` before building.

The `Cargo.lock` remains checked in and should be treated as the
authoritative record of the full dependency graph for reproducibility.

## Deployed on Stellar testnet

| Contract | Contract ID |
|---|---|
| `mergefi-escrow` | `CAY77D2SFDVQYONSPYHOEWARE3UIWQDYHWWI2WXNPFBLBKR2Q4GEWXFB` |
| `mergefi-milestones` | `CBBRLSL6TM6XCNP2XBVT4GFHJ3NNPFKI2BCZQJ4U3TI7GV7DO2F2HG6F` |
| `mergefi-maintenance-pool` | `CD46U7WTEM2I77TXQI2VIBRQXOHEFEYYR2XFA7OVGTXX5M2F7Z3ZQOX2` |

These contract IDs are published alongside the reproducible-build recipe
above so that anyone can independently verify the deployed bytecode
matches the published source.
