import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { parseArg, parseInvokeCli } from "./invoke-args.mjs";
import { Keypair } from "@stellar/stellar-sdk";

const execFileAsync = promisify(execFile);

describe("scripts/invoke.mjs CLI argument validation", () => {
  test("fails with exit code 1 when no arguments or env vars provided", async () => {
    try {
      await execFileAsync("node", ["scripts/invoke.mjs"], {
        env: { ...process.env, INVOKER_SECRET: "", DEPLOYER_SECRET: "" },
      });
      assert.fail("Expected process to exit with non-zero code");
    } catch (err) {
      assert.strictEqual(err.code, 1);
      assert.match(err.stderr, /Usage: INVOKER_SECRET=S/);
    }
  });

  test("fails with exit code 1 when insufficient positional arguments provided", async () => {
    try {
      await execFileAsync("node", ["scripts/invoke.mjs", "SDUMMYSECRET", "C123"], {
        env: { ...process.env, INVOKER_SECRET: "", DEPLOYER_SECRET: "" },
      });
      assert.fail("Expected process to exit with non-zero code");
    } catch (err) {
      assert.strictEqual(err.code, 1);
      assert.match(err.stderr, /Usage/);
    }
  });
});

describe("scripts/invoke.mjs parseArg unit tests", () => {
  test("parses address correctly", () => {
    const validAddress = Keypair.fromRawEd25519Seed(Buffer.alloc(32, 7)).publicKey();
    const scVal = parseArg(`address:${validAddress}`);
    assert.ok(scVal);
  });

  test("parses u32 correctly", () => {
    const scVal = parseArg("u32:250");
    assert.ok(scVal);
  });

  test("parses u64 correctly", () => {
    const scVal = parseArg("u64:1000000");
    assert.ok(scVal);
  });

  test("parses i128 correctly", () => {
    const scVal = parseArg("i128:50000000000");
    assert.ok(scVal);
  });

  test("parses none / void correctly", () => {
    const scVal = parseArg("none");
    assert.ok(scVal);
  });

  test("throws error for unknown argument type", () => {
    assert.throws(
      () => parseArg("invalid_type:123"),
      /Unknown arg type: invalid_type/
    );
  });
});


describe("scripts/invoke.mjs contract context", () => {
  test("supports a flagged contract kind with the existing environment secret", () => {
    const parsed = parseInvokeCli(
      ["--contract-kind=milestones", "CID", "release", "u64:5"],
      { INVOKER_SECRET: "IN_MEMORY_TEST_ONLY" },
    );
    assert.deepEqual(parsed, {
      secret: "IN_MEMORY_TEST_ONLY", contractId: "CID", method: "release",
      args: ["u64:5"], contractKind: "milestones",
    });
  });

  test("keeps the positional secret and accepts a flag after method", () => {
    const parsed = parseInvokeCli(
      ["SECRET", "CID", "withdraw", "--contract-kind=maintenancePool", "u64:2"],
      {},
    );
    assert.deepEqual(parsed, {
      secret: "SECRET", contractId: "CID", method: "withdraw",
      args: ["u64:2"], contractKind: "maintenancePool",
    });
  });

  test("preserves the legacy invocation and supports an env contract hint", () => {
    assert.equal(parseInvokeCli(["CID", "deposit"], { INVOKER_SECRET: "TEST" }).contractKind, undefined);
    assert.equal(parseInvokeCli(["CID", "deposit"], {
      INVOKER_SECRET: "TEST", INVOKER_CONTRACT_KIND: "escrow",
    }).contractKind, "escrow");
  });

  test("rejects an unknown contract kind and repeated flags before sending", () => {
    assert.throws(() => parseInvokeCli(["--contract-kind=other", "C", "f"], {}), /Invalid contract kind/);
    assert.throws(() => parseInvokeCli(["--contract-kind", "escrow", "C", "f"], {}), /--contract-kind=<kind>/);
    assert.throws(() => parseInvokeCli([
      "--contract-kind=escrow", "--contract-kind=milestones", "C", "f",
    ], {}), /only once/);
  });
});
