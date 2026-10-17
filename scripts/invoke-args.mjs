// Pure invocation parsing lives outside invoke.mjs so tests can import it
// without attempting to send a network transaction.
import { Address, nativeToScVal } from "@stellar/stellar-sdk";
import { errorMappings } from "./error-codes.mjs";

export const CONTRACT_KINDS = Object.freeze(Object.keys(errorMappings));

/**
 * @param {string[]} argv - CLI tokens after the node/script prefix
 * @param {NodeJS.ProcessEnv | Record<string, string | undefined>} env
 */
export function parseInvokeCli(argv, env = process.env) {
  const tokens = [];
  let contractKind = env.INVOKER_CONTRACT_KIND || undefined;
  let seenFlag = false;

  for (const token of argv) {
    if (token === "--contract-kind") {
      throw new Error("Specify contract kind as --contract-kind=<kind>");
    }
    if (token.startsWith("--contract-kind=")) {
      if (seenFlag) throw new Error("Specify --contract-kind only once");
      contractKind = token.slice("--contract-kind=".length);
      seenFlag = true;
    } else {
      tokens.push(token);
    }
  }

  if (contractKind !== undefined && !Object.hasOwn(errorMappings, contractKind)) {
    throw new Error(
      `Invalid contract kind "${contractKind}"; choose ${CONTRACT_KINDS.join(", ")}`,
    );
  }

  const envSecret = env.INVOKER_SECRET || env.DEPLOYER_SECRET;
  const [secret, contractId, method, ...args] = envSecret
    ? [envSecret, ...tokens]
    : tokens;

  return { secret, contractId, method, args, contractKind };
}

export function parseArg(raw) {
  if (raw === "none") return nativeToScVal(null);

  const [type, value] = raw.split(":");
  if (type === "address") return nativeToScVal(new Address(value), { type: "address" });
  if (type === "u32") return nativeToScVal(parseInt(value, 10), { type: "u32" });
  if (type === "u64") return nativeToScVal(BigInt(value), { type: "u64" });
  if (type === "i128") return nativeToScVal(BigInt(value), { type: "i128" });
  if (type === "none" || raw === "none") return nativeToScVal(null, { type: "void" });
  throw new Error(`Unknown arg type: ${type}`);
}
