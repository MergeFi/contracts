import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  Keypair,
  TransactionBuilder,
  Networks,
  BASE_FEE,
  Operation,
  Address,
  nativeToScVal,
  rpc,
} from "@stellar/stellar-sdk";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const RPC_URL = "https://soroban-testnet.stellar.org";
const NETWORK_PASSPHRASE = Networks.TESTNET;

const server = new rpc.Server(RPC_URL);
const deployerSecret = process.argv[2];
const wasmPath = process.argv[3];
const admin = process.argv[4];
const treasury = process.argv[5];
const feeBps = process.argv[6];
const contractName = process.argv[7] ?? "contract";

if (!deployerSecret || !wasmPath || !admin || !treasury || feeBps === undefined) {
  console.error(
    "Usage: node deploy.mjs <secret> <wasm-path> <admin> <treasury> <fee-bps> [name]",
  );
  process.exit(1);
}

const kp = Keypair.fromSecret(deployerSecret);
const parsedFeeBps = Number.parseInt(feeBps, 10);
if (!Number.isInteger(parsedFeeBps) || parsedFeeBps < 0 || parsedFeeBps > 10_000) {
  throw new Error("fee-bps must be an integer between 0 and 10000");
}
if (admin !== kp.publicKey()) {
  throw new Error(
    "admin must match the supplied secret key because the constructor requires admin authorization",
  );
}

const constructorArgs = [
  nativeToScVal(new Address(admin), { type: "address" }),
  nativeToScVal(new Address(treasury), { type: "address" }),
  nativeToScVal(parsedFeeBps, { type: "u32" }),
];

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

  // 2. Create and initialize the instance atomically via __constructor.
  const account2 = await server.getAccount(kp.publicKey());
  const createTx = new TransactionBuilder(account2, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(
      Operation.createCustomContract({
        address: new Address(kp.publicKey()),
        wasmHash,
        constructorArgs,
        salt: Buffer.from(
          Array.from({ length: 32 }, () => Math.floor(Math.random() * 256)),
        ),
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
