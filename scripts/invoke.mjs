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

const RPC_URL = "https://soroban-testnet.stellar.org";
const NETWORK_PASSPHRASE = Networks.TESTNET;
const server = new rpc.Server(RPC_URL);

const [, , secret, contractId, method, ...args] = process.argv;
if (!secret || !contractId || !method) {
  console.error("Usage: node invoke.mjs <secret> <contractId> <method> [args as address:G... or u32:123]");
  process.exit(1);
}

function parseArg(raw) {
  const [type, value] = raw.split(":");
  if (type === "address") return nativeToScVal(new Address(value), { type: "address" });
  if (type === "u32") return nativeToScVal(parseInt(value, 10), { type: "u32" });
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
  console.log(`${method} succeeded. hash: ${sendResult.hash}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
