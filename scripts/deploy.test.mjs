import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

describe("scripts/deploy.mjs CLI argument validation", () => {
  test("fails with exit code 1 when no arguments or env vars provided", async () => {
    try {
      await execFileAsync("node", ["scripts/deploy.mjs"], {
        env: { ...process.env, DEPLOYER_SECRET: "" },
      });
      assert.fail("Expected process to exit with non-zero code");
    } catch (err) {
      assert.strictEqual(err.code, 1);
      assert.match(err.stderr, /Usage: DEPLOYER_SECRET=S/);
    }
  });

  test("rejects a positional secret instead of treating it as credentials", async () => {
    try {
      await execFileAsync("node", ["scripts/deploy.mjs", "SDUMMYSECRET", "fake.wasm"], {
        env: { ...process.env, DEPLOYER_SECRET: "" },
      });
      assert.fail("Expected process to exit with non-zero code");
    } catch (err) {
      assert.strictEqual(err.code, 1);
      assert.match(err.stderr, /Usage: DEPLOYER_SECRET=S/);
      assert.doesNotMatch(err.stderr, /insecure fallback/);
    }
  });

  test("fails with exit code 1 when wasm path is provided without DEPLOYER_SECRET", async () => {
    try {
      await execFileAsync("node", ["scripts/deploy.mjs", "target/wasm.wasm"], {
        env: { ...process.env, DEPLOYER_SECRET: "" },
      });
      assert.fail("Expected process to exit with non-zero code");
    } catch (err) {
      assert.strictEqual(err.code, 1);
      assert.match(err.stderr, /Usage: DEPLOYER_SECRET=S/);
    }
  });

  test("fails with exit code 1 when DEPLOYER_SECRET is provided without wasm path", async () => {
    try {
      await execFileAsync("node", ["scripts/deploy.mjs"], {
        env: { ...process.env, DEPLOYER_SECRET: "SDUMMYSECRET" },
      });
      assert.fail("Expected process to exit with non-zero code");
    } catch (err) {
      assert.strictEqual(err.code, 1);
      assert.match(err.stderr, /Usage: DEPLOYER_SECRET=S/);
    }
  });
});
