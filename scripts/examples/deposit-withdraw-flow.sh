#!/usr/bin/env bash
# End-to-end integration example: initialize -> deposit -> withdraw against
# the mergefi-maintenance-pool contract, chaining the same
# `node scripts/invoke.mjs` calls a real integrator would use.
#
# This is deliberately the deposit/withdraw flow rather than escrow's
# fund/release: release's `recipients` argument is a Vec<(Address, u32)>,
# which scripts/invoke.mjs's CLI argument parser can't encode yet (that's
# the separate, already-filed "no generated TypeScript bindings" /
# argument-encoding gap, tracked independently of this example). Every call
# below uses only address/u64/i128 arguments, which invoke.mjs already
# supports, so the whole flow is actually runnable as shown.
#
# Usage:
#   ADMIN_SECRET=S... \
#   ORACLE_SECRET=S... \
#   SPONSOR_SECRET=S... \
#   TOKEN=C... \
#   CONTRACT_ID=C... \
#     ./scripts/examples/deposit-withdraw-flow.sh
#
# CONTRACT_ID may be omitted to deploy a fresh mergefi-maintenance-pool
# instance as part of the flow (requires WASM_PATH to point at the built
# .wasm, see README's "Build, test, deploy" section).
#
# All variables below default to placeholders — replace them with funded
# testnet keys/addresses before running for real, or export the
# corresponding env vars.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

ADMIN_SECRET="${ADMIN_SECRET:?Set ADMIN_SECRET to a funded testnet secret key}"
SPONSOR_SECRET="${SPONSOR_SECRET:-$ADMIN_SECRET}"
ORACLE_SECRET="${ORACLE_SECRET:-$ADMIN_SECRET}"
TOKEN="${TOKEN:?Set TOKEN to a Stellar Asset Contract address (e.g. a testnet USDC SAC)}"
WASM_PATH="${WASM_PATH:-target/wasm32v1-none/release/mergefi_maintenance_pool.wasm}"
POOL_ID="${POOL_ID:-1}"
DEPOSIT_AMOUNT="${DEPOSIT_AMOUNT:-1000000}"
WITHDRAW_AMOUNT="${WITHDRAW_AMOUNT:-500000}"
FEE_BPS="${FEE_BPS:-250}"

admin_pub() {
  node -e "const {Keypair}=require('@stellar/stellar-sdk'); console.log(Keypair.fromSecret(process.argv[1]).publicKey())" "$1"
}

ADMIN_ADDRESS="$(admin_pub "$ADMIN_SECRET")"
ORACLE_ADDRESS="$(admin_pub "$ORACLE_SECRET")"
TREASURY_ADDRESS="${TREASURY_ADDRESS:-$ADMIN_ADDRESS}"
SPONSOR_ADDRESS="$(admin_pub "$SPONSOR_SECRET")"

if [ -z "${CONTRACT_ID:-}" ]; then
  echo "==> No CONTRACT_ID given, deploying a fresh mergefi-maintenance-pool instance"
  CONTRACT_ID="$(node scripts/deploy.mjs "$ADMIN_SECRET" "$WASM_PATH" maintenance-pool | tee /dev/stderr | grep -oE '[A-Z0-9]{56}')"
fi
echo "==> Using contract: $CONTRACT_ID"

echo "==> 1/3 initialize(admin=$ADMIN_ADDRESS, oracle=$ORACLE_ADDRESS, treasury=$TREASURY_ADDRESS, fee_bps=$FEE_BPS)"
node scripts/invoke.mjs "$ADMIN_SECRET" "$CONTRACT_ID" initialize \
  "address:$ADMIN_ADDRESS" "address:$ORACLE_ADDRESS" "address:$TREASURY_ADDRESS" "u32:$FEE_BPS" "none"

echo "==> 2/3 deposit(pool_id=$POOL_ID, sponsor=$SPONSOR_ADDRESS, token=$TOKEN, amount=$DEPOSIT_AMOUNT)"
node scripts/invoke.mjs "$SPONSOR_SECRET" "$CONTRACT_ID" deposit \
  "u64:$POOL_ID" "address:$SPONSOR_ADDRESS" "address:$TOKEN" "i128:$DEPOSIT_AMOUNT"

echo "==> 3/3 withdraw(pool_id=$POOL_ID, recipient=$ADMIN_ADDRESS, amount=$WITHDRAW_AMOUNT)"
node scripts/invoke.mjs "$ORACLE_SECRET" "$CONTRACT_ID" withdraw \
  "u64:$POOL_ID" "address:$ADMIN_ADDRESS" "i128:$WITHDRAW_AMOUNT"

echo "==> Done. Query the pool's remaining balance with:"
echo "    node scripts/invoke.mjs $ADMIN_SECRET $CONTRACT_ID get_pool u64:$POOL_ID"
