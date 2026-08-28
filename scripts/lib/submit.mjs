// Shared transaction submit-and-poll helper for deploy.mjs and invoke.mjs.
//
// Soroban RPC's sendTransaction can return more than just "PENDING" or
// "ERROR": "TRY_AGAIN_LATER" means the node's own submission queue was full
// and the transaction was NOT accepted (distinct from a hard failure — the
// same transaction should be resent as-is after a short delay), and
// "DUPLICATE" means a transaction with this exact hash was already
// submitted (so it's already being processed/finalized and can go straight
// to polling). Neither used to be handled, which meant a TRY_AGAIN_LATER
// response fell through into polling getTransaction for a hash that may
// never actually have been accepted, producing a confusing hang.
export async function submitAndWait(
  server,
  kp,
  tx,
  { maxSendRetries = 5, retryDelayMs = 2000, pollDelayMs = 1500 } = {},
) {
  const prepared = await server.prepareTransaction(tx);
  prepared.sign(kp);

  let sendResult;
  let attempt = 0;
  for (;;) {
    sendResult = await server.sendTransaction(prepared);

    if (sendResult.status === "ERROR") {
      throw new Error(`Send failed: ${JSON.stringify(sendResult.errorResult)}`);
    }

    if (sendResult.status === "TRY_AGAIN_LATER") {
      attempt += 1;
      if (attempt > maxSendRetries) {
        throw new Error(
          `sendTransaction kept returning TRY_AGAIN_LATER after ${maxSendRetries} retries (node submission queue stayed full)`,
        );
      }
      console.warn(
        `sendTransaction returned TRY_AGAIN_LATER (submission queue full); retrying in ${retryDelayMs}ms (attempt ${attempt}/${maxSendRetries})`,
      );
      await new Promise((r) => setTimeout(r, retryDelayMs));
      continue;
    }

    if (sendResult.status === "DUPLICATE") {
      console.warn(
        `sendTransaction returned DUPLICATE; tx ${sendResult.hash} was already submitted, polling for its result`,
      );
    }

    break;
  }

  let getResult = await server.getTransaction(sendResult.hash);
  while (getResult.status === "NOT_FOUND") {
    await new Promise((r) => setTimeout(r, pollDelayMs));
    getResult = await server.getTransaction(sendResult.hash);
  }
  if (getResult.status !== "SUCCESS") {
    throw new Error(`Tx failed: ${JSON.stringify(getResult)}`);
  }
  return { sendResult, getResult };
}
