import {
  Keypair,
  TransactionBuilder,
  Networks,
  BASE_FEE,
  Contract,
  rpc,
} from "@stellar/stellar-sdk";
import { submitAndWait } from "./lib/submit.mjs";
import { formatError } from "./error-codes.mjs";
import { parseArg, parseInvokeCli } from "./invoke-args.mjs";

const RPC_URL = process.env.RPC_URL || "https://soroban-testnet.stellar.org";
const NETWORK_PASSPHRASE = process.env.NETWORK_PASSPHRASE || Networks.TESTNET;
const server = new rpc.Server(RPC_URL);

let invocation;
try {
  invocation = parseInvokeCli(process.argv.slice(2), process.env);
} catch (err) {
  console.error(err.message);
  process.exit(1);
}

const { secret, contractId, method, args, contractKind } = invocation;

if (!secret || !contractId || !method) {
  console.error("Usage: INVOKER_SECRET=S... node invoke.mjs [--contract-kind=escrow|milestones|maintenancePool] <contractId> <method> [args...]");
  console.error("   or: node invoke.mjs [--contract-kind=...] <secret> <contractId> <method> [args...] (insecure fallback)");
  process.exit(1);
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
  console.error(formatError(err, contractKind));
  process.exit(1);
});
