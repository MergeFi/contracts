import {
  Keypair,
  TransactionBuilder,
  Networks,
  BASE_FEE,
  Contract,
  Address,
  nativeToScVal,
  rpc,
} from "@stellar/stellar-sdk";
import { submitAndWait } from "./lib/submit.mjs";

const RPC_URL = process.env.RPC_URL || "https://soroban-testnet.stellar.org";
const NETWORK_PASSPHRASE = process.env.NETWORK_PASSPHRASE || Networks.TESTNET;
const server = new rpc.Server(RPC_URL);

const [, , secret, contractId, method, ...args] = process.argv;
if (!secret || !contractId || !method) {
  console.error("Usage: node invoke.mjs <secret> <contractId> <method> [args as address:G..., u32:123, u64:123, i128:123, or none]");
  process.exit(1);
}

function parseArg(raw) {
  if (raw === "none") return nativeToScVal(null);

  const [type, value] = raw.split(":");
  if (type === "address") return nativeToScVal(new Address(value), { type: "address" });
  if (type === "u32") return nativeToScVal(parseInt(value, 10), { type: "u32" });
  if (type === "u64") return nativeToScVal(BigInt(value), { type: "u64" });
  if (type === "i128") return nativeToScVal(BigInt(value), { type: "i128" });
  throw new Error(`Unknown arg type: ${type}`);
}

const kp = Keypair.fromSecret(secret);
const contract = new Contract(contractId);
const scArgs = args.map(parseArg);

async function main() {
  const account = await server.getAccount(kp.publicKey());
  const tx = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(contract.call(method, ...scArgs))
    .setTimeout(60)
    .build();

  const { sendResult } = await submitAndWait(server, kp, tx);
  console.log(`${method} succeeded. hash: ${sendResult.hash}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
