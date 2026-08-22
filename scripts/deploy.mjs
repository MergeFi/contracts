import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { fileURLToPath } from "node:url";
import {
  Keypair,
  TransactionBuilder,
  Networks,
  BASE_FEE,
  Operation,
  Address,
  rpc,
} from "@stellar/stellar-sdk";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const NETWORK_PASSPHRASE =
  process.env.STELLAR_NETWORK_PASSPHRASE ||
  (process.env.STELLAR_NETWORK === "mainnet" ? Networks.PUBLIC : Networks.TESTNET);

const RPC_URL =
  process.env.STELLAR_RPC_URL ||
  (process.env.STELLAR_NETWORK === "mainnet"
    ? "https://soroban-rpc.mainnet.stellar.org"
    : "https://soroban-testnet.stellar.org");

const server = new rpc.Server(RPC_URL);

const deployerSecret =
  process.env.MERGEFI_DEPLOYER_SECRET ||
  process.env.STELLAR_SECRET_KEY ||
  process.argv[2];

const wasmPath = process.env.MERGEFI_DEPLOYER_SECRET || process.env.STELLAR_SECRET_KEY
  ? process.argv[2]
  : process.argv[3];

const contractName = (process.env.MERGEFI_DEPLOYER_SECRET || process.env.STELLAR_SECRET_KEY
  ? process.argv[3]
  : process.argv[4]) ?? "contract";

if (!deployerSecret || !wasmPath) {
  console.error("Usage: node deploy.mjs [secret] <wasm-path> [name]");
  console.error("Or provide secret via MERGEFI_DEPLOYER_SECRET / STELLAR_SECRET_KEY environment variable.");
  process.exit(1);
}

const kp = Keypair.fromSecret(deployerSecret);

async function submitAndWait(tx) {
  const prepared = await server.prepareTransaction(tx);
  prepared.sign(kp);
  const sendResult = await server.sendTransaction(prepared);
  if (sendResult.status === "ERROR") {
    throw new Error(`Send failed: ${JSON.stringify(sendResult.errorResult)}`);
  }
  let getResult = await server.getTransaction(sendResult.hash);
  while (getResult.status === "NOT_FOUND") {
    await new Promise((r) => setTimeout(r, 1500));
    getResult = await server.getTransaction(sendResult.hash);
  }
  if (getResult.status !== "SUCCESS") {
    throw new Error(`Tx failed: ${JSON.stringify(getResult)}`);
  }
  return getResult;
}

async function main() {
  const wasmBuffer = fs.readFileSync(path.resolve(process.cwd(), wasmPath));

  const account = await server.getAccount(kp.publicKey());

  // 1. Upload the contract WASM, get its hash.
  const uploadTx = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(Operation.uploadContractWasm({ wasm: wasmBuffer }))
    .setTimeout(60)
    .build();

  const uploadResult = await submitAndWait(uploadTx);
  const wasmHash = uploadResult.returnValue.bytes();
  console.log(`[${contractName}] wasm uploaded, hash: ${wasmHash.toString("hex")}`);

  // 2. Create the contract instance from that wasm hash.
  const account2 = await server.getAccount(kp.publicKey());
  const createTx = new TransactionBuilder(account2, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(
      Operation.createCustomContract({
        address: new Address(kp.publicKey()),
        wasmHash,
        salt: crypto.randomBytes(32),
      }),
    )
    .setTimeout(60)
    .build();

  const createResult = await submitAndWait(createTx);
  const contractAddress = Address.fromScAddress(
    createResult.returnValue.address(),
  ).toString();

  console.log(`[${contractName}] deployed at: ${contractAddress}`);
  return contractAddress;
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
