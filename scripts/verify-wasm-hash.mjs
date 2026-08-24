#!/usr/bin/env node
/**
 * Verify that the locally built contract WASM matches what is actually
 * deployed on Stellar testnet, byte-for-byte.
 *
 * For each of the three contracts it:
 *   1. reads the WASM hash recorded in the ledger for the deployed contract
 *      (the contract instance entry carries the executable WASM hash — this
 *      is the hash of the exact bytes uploaded at deploy time);
 *   2. reads `target/wasm32v1-none/release/<contract>.wasm` from a local
 *      `cargo build --target wasm32v1-none --release`;
 *   3. compares them and reports match / mismatch.
 *
 * Usage (from the repo root, after `make build`):
 *   node scripts/verify-wasm-hash.mjs
 *
 * Exit code is 0 only if every contract matches. A mismatch means the
 * locally built bytecode is not what is running on testnet — see the README's
 * "Reproducible builds" section for what this implies and how to reproduce
 * the deployed bytecode exactly.
 */
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { Address, rpc, xdr } from "@stellar/stellar-sdk";

const RPC_URL = "https://soroban-testnet.stellar.org";
const WASM_DIR = path.resolve("target/wasm32v1-none/release");

const CONTRACTS = [
  ["mergefi-escrow", "mergefi_escrow.wasm", "CAY77D2SFDVQYONSPYHOEWARE3UIWQDYHWWI2WXNPFBLBKR2Q4GEWXFB"],
  ["mergefi-milestones", "mergefi_milestones.wasm", "CBBRLSL6TM6XCNP2XBVT4GFHJ3NNPFKI2BCZQJ4U3TI7GV7DO2F2HG6F"],
  ["mergefi-maintenance-pool", "mergefi_maintenance_pool.wasm", "CD46U7WTEM2I77TXQI2VIBRQXOHEFEYYR2XFA7OVGTXX5M2F7Z3ZQOX2"],
];

const server = new rpc.Server(RPC_URL);

// The contract instance ledger entry (Protocol 22+) carries the executable
// WASM hash. Ledger key: ContractData { contract, key: LedgerKeyContractInstance,
// durability: Persistent }.
function instanceLedgerKey(contractId) {
  return xdr.LedgerKey.contractData(
    new xdr.LedgerKeyContractData({
      contract: new Address(contractId).toScAddress(),
      key: xdr.ScVal.scvLedgerKeyContractInstance(),
      durability: xdr.ContractDataDurability.persistent(),
    }),
  );
}

async function onChainWasmHash(contractId) {
  const res = await server.getLedgerEntries(instanceLedgerKey(contractId));
  const entry = res.entries?.[0];
  if (!entry) {
    throw new Error(`no instance entry found for ${contractId}`);
  }
  const executable = entry.val.contractData().val().instance().executable();
  return executable.wasmHash().toString("hex");
}

let allMatch = true;
for (const [name, wasmFile, contractId] of CONTRACTS) {
  const wasmPath = path.join(WASM_DIR, wasmFile);
  if (!fs.existsSync(wasmPath)) {
    console.log(`✗ ${name}: local wasm not found at ${wasmPath} (run \`make build\` first)`);
    allMatch = false;
    continue;
  }
  const localHash = crypto
    .createHash("sha256")
    .update(fs.readFileSync(wasmPath))
    .digest("hex");
  let deployedHash;
  try {
    deployedHash = await onChainWasmHash(contractId);
  } catch (err) {
    console.log(`✗ ${name}: could not read on-chain hash: ${err.message}`);
    allMatch = false;
    continue;
  }
  const match = localHash === deployedHash;
  if (!match) allMatch = false;
  console.log(
    `${match ? "✓" : "✗"} ${name}`,
    `\n    deployed on testnet: ${deployedHash}`,
    `\n    local build:         ${localHash}`,
    match ? "" : "\n    MISMATCH — local bytecode differs from what is deployed on testnet",
  );
}

process.exit(allMatch ? 0 : 1);
