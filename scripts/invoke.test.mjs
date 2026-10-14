import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { parseArg } from "./invoke.mjs";

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
    const validAddress = "GB3KJPLFUYN5VL6R3GU3EGCGVCKFDSD7BEDX42HWG5BWFKB3KQGJJRMA";
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
