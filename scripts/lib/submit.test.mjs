import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { submitAndWait } from "./submit.mjs";

describe("scripts/lib/submit.mjs submitAndWait", () => {
  const fakeKp = {
    publicKey: () => "GBZXN7PIRZGNMHGA72BLBHNTFAJUR3NGMHHCC5YDHJL3PPCHQSTFU7H6",
  };
  const fakeTx = { id: "tx_raw" };

  function createFakePrepared() {
    let signed = false;
    return {
      sign(kp) {
        signed = true;
        assert.strictEqual(kp, fakeKp);
      },
      isSigned() {
        return signed;
      },
    };
  }

  test("submits and polls successfully (PENDING -> NOT_FOUND -> SUCCESS)", async () => {
    let preparedTx;
    let pollCount = 0;
    const fakeServer = {
      async prepareTransaction(tx) {
        assert.strictEqual(tx, fakeTx);
        preparedTx = createFakePrepared();
        return preparedTx;
      },
      async sendTransaction(prepared) {
        assert.strictEqual(prepared, preparedTx);
        assert.strictEqual(prepared.isSigned(), true);
        return { status: "PENDING", hash: "hash123" };
      },
      async getTransaction(hash) {
        assert.strictEqual(hash, "hash123");
        pollCount += 1;
        if (pollCount === 1) {
          return { status: "NOT_FOUND" };
        }
        return { status: "SUCCESS", hash: "hash123", result: "ok" };
      },
    };

    const result = await submitAndWait(fakeServer, fakeKp, fakeTx, {
      maxSendRetries: 3,
      retryDelayMs: 1,
      pollDelayMs: 1,
    });

    assert.strictEqual(result.sendResult.status, "PENDING");
    assert.strictEqual(result.getResult.status, "SUCCESS");
    assert.strictEqual(pollCount, 2);
  });

  test("retries when sendTransaction returns TRY_AGAIN_LATER and then succeeds", async () => {
    let sendAttempts = 0;
    const fakeServer = {
      async prepareTransaction() {
        return createFakePrepared();
      },
      async sendTransaction() {
        sendAttempts += 1;
        if (sendAttempts < 3) {
          return { status: "TRY_AGAIN_LATER" };
        }
        return { status: "PENDING", hash: "hash_retry" };
      },
      async getTransaction(hash) {
        assert.strictEqual(hash, "hash_retry");
        return { status: "SUCCESS", hash: "hash_retry" };
      },
    };

    const result = await submitAndWait(fakeServer, fakeKp, fakeTx, {
      maxSendRetries: 5,
      retryDelayMs: 5,
      pollDelayMs: 1,
    });

    assert.strictEqual(sendAttempts, 3);
    assert.strictEqual(result.sendResult.status, "PENDING");
    assert.strictEqual(result.getResult.status, "SUCCESS");
  });

  test("fails when sendTransaction keeps returning TRY_AGAIN_LATER exceeding maxSendRetries", async () => {
    let sendAttempts = 0;
    const fakeServer = {
      async prepareTransaction() {
        return createFakePrepared();
      },
      async sendTransaction() {
        sendAttempts += 1;
        return { status: "TRY_AGAIN_LATER" };
      },
      async getTransaction() {
        assert.fail("getTransaction should not be called if sendTransaction times out");
      },
    };

    await assert.rejects(
      submitAndWait(fakeServer, fakeKp, fakeTx, {
        maxSendRetries: 2,
        retryDelayMs: 1,
        pollDelayMs: 1,
      }),
      /sendTransaction kept returning TRY_AGAIN_LATER after 2 retries/,
    );
    assert.strictEqual(sendAttempts, 3); // initial attempt + 2 retries
  });

  test("handles DUPLICATE sendResult by proceeding directly to polling", async () => {
    const fakeServer = {
      async prepareTransaction() {
        return createFakePrepared();
      },
      async sendTransaction() {
        return { status: "DUPLICATE", hash: "hash_duplicate" };
      },
      async getTransaction(hash) {
        assert.strictEqual(hash, "hash_duplicate");
        return { status: "SUCCESS", hash: "hash_duplicate" };
      },
    };

    const result = await submitAndWait(fakeServer, fakeKp, fakeTx, {
      pollDelayMs: 1,
    });

    assert.strictEqual(result.sendResult.status, "DUPLICATE");
    assert.strictEqual(result.getResult.status, "SUCCESS");
  });

  test("throws when sendTransaction returns ERROR status", async () => {
    const fakeServer = {
      async prepareTransaction() {
        return createFakePrepared();
      },
      async sendTransaction() {
        return { status: "ERROR", errorResult: "tx_bad_auth" };
      },
      async getTransaction() {
        assert.fail("getTransaction should not be called after send error");
      },
    };

    await assert.rejects(
      submitAndWait(fakeServer, fakeKp, fakeTx),
      /Send failed: "tx_bad_auth"/,
    );
  });

  test("throws when getTransaction returns a non-SUCCESS status", async () => {
    const fakeServer = {
      async prepareTransaction() {
        return createFakePrepared();
      },
      async sendTransaction() {
        return { status: "PENDING", hash: "hash_failed_tx" };
      },
      async getTransaction() {
        return { status: "FAILED", errorResult: "tx_failed_execution" };
      },
    };

    await assert.rejects(
      submitAndWait(fakeServer, fakeKp, fakeTx, { pollDelayMs: 1 }),
      /Tx failed: {"status":"FAILED","errorResult":"tx_failed_execution"}/,
    );
  });
});
