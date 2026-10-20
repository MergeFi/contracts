import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { Address, nativeToScVal } from "@stellar/stellar-sdk";
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
      await execFileAsync("node", ["scripts/invoke.mjs", "C123"], {
        env: { ...process.env, INVOKER_SECRET: "SDUMMYSECRET", DEPLOYER_SECRET: "" },
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
    const validAddress = "GB7S3DTWAQVDQMUDJ3P3LVQHFNLDTCF7XZZQ5POJE5X2IMFAZV2ABJBK";
    const scVal = parseArg(`address:${validAddress}`);
    assert.ok(scVal);
  });

  test("parses recipients as a vector of address/u32 tuple vectors", () => {
    const address1 = "GB7S3DTWAQVDQMUDJ3P3LVQHFNLDTCF7XZZQ5POJE5X2IMFAZV2ABJBK";
    const address2 = "GABAUE3AINF6AJCVPCNEWD6ZXARC24FI6B2YSDEQOHOK4V7O3CUA6QM3";
    const recipients = parseArg(`recipients:${address1}=7000,${address2}=3000`).vec();

    assert.equal(recipients.length, 2);
    assert.equal(recipients[0].vec().length, 2);
    assert.equal(
      Address.fromScAddress(recipients[0].vec()[0].address()).toString(),
      address1
    );
    assert.equal(recipients[0].vec()[1].u32(), 7000);
    assert.equal(recipients[1].vec()[1].u32(), 3000);
  });

  test("rejects malformed recipient vectors", () => {
    assert.throws(() => parseArg("recipients:"), /Invalid recipients argument/);
    assert.throws(() => parseArg("recipients:GABC"), /Invalid recipients entry/);
    assert.throws(() => parseArg("recipients:GABC=foo"), /Invalid recipients entry/);
    assert.throws(
      () => parseArg("recipients:GABC=1000=extra"),
      /Invalid recipients entry/
    );
    assert.throws(
      () => parseArg("recipients:GABC=4294967296"),
      /Recipient basis points must fit u32/
    );
  });

  test("parses u32 correctly", () => {
    const scVal = parseArg("u32:250");
    assert.equal(scVal.u32(), 250);
  });

  test("parses u64 correctly", () => {
    const scVal = parseArg("u64:1000000");
    assert.equal(scVal.u64().toBigInt(), 1000000n);
  });

  test("parses i128 correctly", () => {
    const scVal = parseArg("i128:50000000000");
    const expected = nativeToScVal(50000000000n, { type: "i128" });
    assert.deepEqual(scVal.i128(), expected.i128());
  });

  test("parses Some(i128) correctly", () => {
    const scVal = parseArg("some-i128:50000000000");
    const expected = nativeToScVal(50000000000n, { type: "i128" });
    assert.deepEqual(scVal.i128(), expected.i128());
  });

  test("parses bytes32 hex correctly", () => {
    const bytes = "0123456789abcdef".repeat(4);
    const scVal = parseArg(`bytes32:${bytes}`);
    assert.deepEqual(scVal.bytes(), Buffer.from(bytes, "hex"));
  });

  test("rejects bytes32 values that are not exactly 32 bytes", () => {
    assert.throws(() => parseArg("bytes32:abcd"), /exactly 32 bytes/);
  });

  test("parses boolean values and rejects other input", () => {
    assert.equal(parseArg("bool:true").b(), true);
    assert.equal(parseArg("bool:false").b(), false);
    assert.throws(() => parseArg("bool:yes"), /Invalid bool argument/);
  });

  test("parses none as a void value", () => {
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
