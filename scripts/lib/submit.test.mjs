// Tests for scripts/lib/submit.mjs.
//
// Uses plain JS objects as fake RPC servers — no real network calls.
// Covers all sendTransaction status branches (PENDING→SUCCESS,
// TRY_AGAIN_LATER retry, TRY_AGAIN_LATER exhaustion, DUPLICATE, ERROR)
// and all getTransaction outcomes (NOT_FOUND poll loop, SUCCESS, failure).
//
// Run with: node --test scripts/lib/submit.test.mjs
// or via:   npm test  (picks up scripts/lib/*.test.mjs via the glob)
import { test, describe, beforeEach } from "node:test";
import assert from "node:assert/strict";

import { submitAndWait } from "./submit.mjs";

// ---------------------------------------------------------------------------
// Minimal fake keypair — submit.mjs only calls kp via prepared.sign(kp).
// ---------------------------------------------------------------------------
const fakeKp = {};

// ---------------------------------------------------------------------------
// Helper: build a fake server from a sequence of sendTransaction responses
// and a sequence of getTransaction responses.
// ---------------------------------------------------------------------------
function makeServer({ sends, gets }) {
  let sendIndex = 0;
  let getIndex = 0;
  return {
    async prepareTransaction(tx) {
      // Returns a fake prepared tx that tracks sign calls.
      return {
        _tx: tx,
        _signed: false,
        sign(_kp) {
          this._signed = true;
        },
      };
    },
    async sendTransaction(_prepared) {
      if (sendIndex >= sends.length) throw new Error("Unexpected sendTransaction call");
      return sends[sendIndex++];
    },
    async getTransaction(_hash) {
      if (getIndex >= gets.length) throw new Error("Unexpected getTransaction call");
      return gets[getIndex++];
    },
  };
}

// Fake tx object — submit.mjs treats it as opaque (just passes to prepareTransaction).
const fakeTx = { _name: "fakeTx" };

// Use zero delays for all tests so the suite runs instantly.
const noDelay = { retryDelayMs: 0, pollDelayMs: 0 };

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("submitAndWait — PENDING → SUCCESS (happy path)", () => {
  test("prepares, signs, sends, polls and resolves with sendResult + getResult", async () => {
    const server = makeServer({
      sends: [{ status: "PENDING", hash: "abc123" }],
      gets: [{ status: "SUCCESS", resultMetaXdr: "xdr" }],
    });

    const result = await submitAndWait(server, fakeKp, fakeTx, noDelay);

    assert.equal(result.sendResult.status, "PENDING");
    assert.equal(result.sendResult.hash, "abc123");
    assert.equal(result.getResult.status, "SUCCESS");
  });
});

describe("submitAndWait — NOT_FOUND polling loop", () => {
  test("polls getTransaction until status leaves NOT_FOUND", async () => {
    const server = makeServer({
      sends: [{ status: "PENDING", hash: "poll1" }],
      gets: [
        { status: "NOT_FOUND" },
        { status: "NOT_FOUND" },
        { status: "SUCCESS", resultMetaXdr: "xdr" },
      ],
    });

    const result = await submitAndWait(server, fakeKp, fakeTx, noDelay);
    assert.equal(result.getResult.status, "SUCCESS");
  });
});

describe("submitAndWait — TRY_AGAIN_LATER retry", () => {
  test("retries send on TRY_AGAIN_LATER then succeeds", async () => {
    const warnMessages = [];
    const originalWarn = console.warn;
    console.warn = (...args) => warnMessages.push(args.join(" "));

    try {
      const server = makeServer({
        sends: [
          { status: "TRY_AGAIN_LATER", hash: "t1" },
          { status: "TRY_AGAIN_LATER", hash: "t1" },
          { status: "PENDING", hash: "t1" },
        ],
        gets: [{ status: "SUCCESS", resultMetaXdr: "xdr" }],
      });

      const result = await submitAndWait(server, fakeKp, fakeTx, {
        maxSendRetries: 3,
        ...noDelay,
      });

      assert.equal(result.sendResult.status, "PENDING");
      assert.equal(result.getResult.status, "SUCCESS");
      // Should have warned twice (once per TRY_AGAIN_LATER).
      assert.equal(warnMessages.length, 2);
      assert.match(warnMessages[0], /TRY_AGAIN_LATER/);
    } finally {
      console.warn = originalWarn;
    }
  });

  test("throws after maxSendRetries is exhausted", async () => {
    // Suppress the console.warn noise in this test.
    const originalWarn = console.warn;
    console.warn = () => {};

    try {
      const server = makeServer({
        // maxSendRetries defaults to 5, so we need 6 TRY_AGAIN_LATER responses.
        sends: Array(6).fill({ status: "TRY_AGAIN_LATER", hash: "tx" }),
        gets: [],
      });

      await assert.rejects(
        () => submitAndWait(server, fakeKp, fakeTx, { maxSendRetries: 5, ...noDelay }),
        (err) => {
          assert.match(err.message, /TRY_AGAIN_LATER/);
          assert.match(err.message, /5 retries/);
          return true;
        },
      );
    } finally {
      console.warn = originalWarn;
    }
  });

  test("throws on the first attempt when maxSendRetries is 0", async () => {
    const originalWarn = console.warn;
    console.warn = () => {};

    try {
      const server = makeServer({
        sends: [{ status: "TRY_AGAIN_LATER", hash: "tx" }],
        gets: [],
      });

      await assert.rejects(
        () => submitAndWait(server, fakeKp, fakeTx, { maxSendRetries: 0, ...noDelay }),
        /TRY_AGAIN_LATER/,
      );
    } finally {
      console.warn = originalWarn;
    }
  });
});

describe("submitAndWait — DUPLICATE", () => {
  test("skips straight to polling when server returns DUPLICATE", async () => {
    const warnMessages = [];
    const originalWarn = console.warn;
    console.warn = (...args) => warnMessages.push(args.join(" "));

    try {
      const server = makeServer({
        sends: [{ status: "DUPLICATE", hash: "dup99" }],
        gets: [{ status: "SUCCESS", resultMetaXdr: "xdr" }],
      });

      const result = await submitAndWait(server, fakeKp, fakeTx, noDelay);

      assert.equal(result.sendResult.status, "DUPLICATE");
      assert.equal(result.getResult.status, "SUCCESS");
      // Should have emitted exactly one warning mentioning the hash.
      assert.equal(warnMessages.length, 1);
      assert.match(warnMessages[0], /DUPLICATE/);
      assert.match(warnMessages[0], /dup99/);
    } finally {
      console.warn = originalWarn;
    }
  });
});

describe("submitAndWait — ERROR on send", () => {
  test("throws immediately without polling when sendTransaction returns ERROR", async () => {
    const server = makeServer({
      sends: [
        {
          status: "ERROR",
          errorResult: { code: "txBAD_SEQ", message: "bad sequence number" },
        },
      ],
      // gets is empty — should never be reached.
      gets: [],
    });

    await assert.rejects(
      () => submitAndWait(server, fakeKp, fakeTx, noDelay),
      (err) => {
        assert.match(err.message, /Send failed/);
        assert.match(err.message, /txBAD_SEQ/);
        return true;
      },
    );
  });
});

describe("submitAndWait — getTransaction non-SUCCESS terminal status", () => {
  test("throws when getTransaction resolves with FAILED status", async () => {
    const server = makeServer({
      sends: [{ status: "PENDING", hash: "fail1" }],
      gets: [{ status: "FAILED", resultXdr: "errorXdr" }],
    });

    await assert.rejects(
      () => submitAndWait(server, fakeKp, fakeTx, noDelay),
      (err) => {
        assert.match(err.message, /Tx failed/);
        assert.match(err.message, /FAILED/);
        return true;
      },
    );
  });

  test("throws when getTransaction resolves with unknown status", async () => {
    const server = makeServer({
      sends: [{ status: "PENDING", hash: "unk1" }],
      gets: [{ status: "UNKNOWN_STATUS" }],
    });

    await assert.rejects(
      () => submitAndWait(server, fakeKp, fakeTx, noDelay),
      /Tx failed/,
    );
  });
});

describe("submitAndWait — transaction is signed before sending", () => {
  test("prepared.sign is called with the supplied keypair", async () => {
    let receivedKp = null;
    const server = {
      async prepareTransaction(tx) {
        return {
          _tx: tx,
          sign(kp) {
            receivedKp = kp;
          },
        };
      },
      async sendTransaction(_p) {
        return { status: "PENDING", hash: "sig1" };
      },
      async getTransaction(_h) {
        return { status: "SUCCESS", resultMetaXdr: "xdr" };
      },
    };

    const myKp = { _name: "realKp" };
    await submitAndWait(server, myKp, fakeTx, noDelay);
    assert.strictEqual(receivedKp, myKp);
  });
});
