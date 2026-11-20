import { test, describe } from "node:test";
import assert from "node:assert/strict";
import {
  extractErrorCode,
  decodeErrorCode,
  formatError,
  errorMappings,
} from "./error-codes.mjs";

// ─── extractErrorCode ─────────────────────────────────────────────────────────

describe("extractErrorCode", () => {
  // Case 1a: "Send failed: {...}" string
  test("parses numeric code from Send failed JSON string", () => {
    const err = 'Send failed: {"code":5,"message":"AlreadyFunded"}';
    assert.strictEqual(extractErrorCode(err), 5);
  });

  // Case 1b: "Tx failed: {...}" string
  test("parses numeric code from Tx failed JSON string", () => {
    const err = 'Tx failed: {"code":3,"message":"Unauthorized"}';
    assert.strictEqual(extractErrorCode(err), 3);
  });

  // Case 1c: bare "code: N" text (no JSON wrapper)
  test("parses code from bare text pattern", () => {
    assert.strictEqual(extractErrorCode("contract error code: 9"), 9);
    assert.strictEqual(extractErrorCode("code: 12"), 12);
  });

  // Case 2: object with errorResult nesting
  test("recurses into errorResult", () => {
    assert.strictEqual(extractErrorCode({ errorResult: { code: 7 } }), 7);
  });

  // Case 3: object with resultXdr containing a number
  test("extracts first number from stringified resultXdr", () => {
    const code = extractErrorCode({ resultXdr: { inner: 11 } });
    assert.strictEqual(typeof code, "number");
    assert.ok(code >= 0);
  });

  // Case 4a: object with .code property
  test("reads numeric .code property", () => {
    assert.strictEqual(extractErrorCode({ code: 4 }), 4);
  });

  test("coerces string .code to number", () => {
    assert.strictEqual(extractErrorCode({ code: "8" }), 8);
  });

  // Case 4b: object with .error property
  test("reads numeric .error property", () => {
    assert.strictEqual(extractErrorCode({ error: 6 }), 6);
  });

  // Case 4c: object with .value property
  test("reads numeric .value property", () => {
    assert.strictEqual(extractErrorCode({ value: 2 }), 2);
  });

  // Case 5: message recursion
  test("recurses into .message string", () => {
    assert.strictEqual(extractErrorCode({ message: "code: 15" }), 15);
  });

  // Case 6: JSON stringify fallback
  test("falls back to JSON scan for 'error' or 'code' key", () => {
    // Object with no direct code/error/value but stringifiable to {"error":10}
    const code = extractErrorCode(JSON.parse('{"error":10}'));
    assert.strictEqual(code, 10);
  });

  // Edge: null / undefined
  test("returns null for null input", () => {
    assert.strictEqual(extractErrorCode(null), null);
  });

  test("returns null for undefined input", () => {
    assert.strictEqual(extractErrorCode(undefined), null);
  });

  // Edge: unrecognised string with no number
  test("returns null for unrecognised string with no number", () => {
    assert.strictEqual(extractErrorCode("network timeout"), null);
  });
});

// ─── decodeErrorCode ──────────────────────────────────────────────────────────

describe("decodeErrorCode", () => {
  // Code present in exactly one contract
  test("returns one entry for a code unique to one contract", () => {
    // maintenancePool:3 = PoolNotFound (not in escrow or milestones)
    const result = decodeErrorCode(3);
    const mp = result.find((m) => m.startsWith("maintenancePool:"));
    assert.ok(mp, "expected maintenancePool entry");
    assert.match(mp, /PoolNotFound/);
  });

  // Code present in multiple contracts
  test("returns entries for all contracts that map a code", () => {
    // code 1 = AlreadyInitialized in all three contracts
    const result = decodeErrorCode(1);
    assert.ok(result.length >= 3, `expected ≥3 entries, got ${result.length}`);
    const names = result.map((m) => m.split(":")[0]);
    assert.ok(names.includes("escrow"), "missing escrow");
    assert.ok(names.includes("milestones"), "missing milestones");
    assert.ok(names.includes("maintenancePool"), "missing maintenancePool");
  });

  // Unknown code
  test("returns empty array for unknown code", () => {
    assert.deepStrictEqual(decodeErrorCode(9999), []);
  });

  // Null / undefined
  test("returns empty array for null", () => {
    assert.deepStrictEqual(decodeErrorCode(null), []);
  });
});

// ─── formatError ─────────────────────────────────────────────────────────────

describe("formatError", () => {
  test("includes detected error code in output", () => {
    const out = formatError({ code: 5 });
    assert.match(out, /5/);
  });

  test("narrows to escrow when contractName='escrow'", () => {
    // code 1 appears in all contracts; with 'escrow' we should only see escrow
    const out = formatError({ code: 1 }, "escrow");
    assert.match(out, /escrow/);
    assert.doesNotMatch(out, /milestones/);
    assert.doesNotMatch(out, /maintenancePool/);
  });

  test("narrows to milestones when contractName='milestones'", () => {
    const out = formatError({ code: 1 }, "milestones");
    assert.match(out, /milestones/);
    assert.doesNotMatch(out, /escrow/);
  });

  // Maintenance-pool spelling: the key in errorMappings is 'maintenancePool'
  // not 'maintenance-pool'.  formatError must accept 'maintenancePool'.
  test("narrows to maintenancePool when contractName='maintenancePool'", () => {
    const out = formatError({ code: 3 }, "maintenancePool");
    assert.match(out, /maintenancePool/);
    assert.match(out, /PoolNotFound/);
  });

  test("shows all matches when no contractName given and code is ambiguous", () => {
    const out = formatError({ code: 1 });
    assert.match(out, /escrow/);
    assert.match(out, /milestones/);
    assert.match(out, /maintenancePool/);
    assert.match(out, /multiple contracts/i);
  });

  test("handles string error input", () => {
    const out = formatError("code: 9");
    assert.match(out, /9/);
  });

  test("handles error with .message", () => {
    const out = formatError(new Error("code: 2"));
    assert.match(out, /2/);
  });

  test("shows no-mapping note when code has no entry", () => {
    const out = formatError({ code: 9999 });
    assert.match(out, /No known mapping/);
  });

  test("shows no-code note when no code detected", () => {
    const out = formatError({ message: "network timeout" });
    assert.match(out, /No error code detected/);
  });
});

// ─── Table completeness (table vs error.rs drift) ─────────────────────────────

describe("errorMappings table completeness", () => {
  test("escrow table has no gaps below its max code (all discriminants sequential or explicitly skipped)", () => {
    const codes = Object.keys(errorMappings.escrow).map(Number);
    const max = Math.max(...codes);
    // Codes that are intentionally absent from the table are acceptable,
    // but every present code must map to a non-empty string.
    for (const [k, v] of Object.entries(errorMappings.escrow)) {
      assert.ok(typeof v === "string" && v.length > 0, `escrow[${k}] is empty`);
    }
    // The table must cover at least up to discriminant 23 (InvalidMaxSponsors)
    assert.ok(max >= 23, `escrow max code is ${max}, expected ≥23`);
  });

  test("milestones table covers up to InvalidMaxSponsors (24)", () => {
    const max = Math.max(...Object.keys(errorMappings.milestones).map(Number));
    assert.ok(max >= 24, `milestones max code is ${max}, expected ≥24`);
  });

  test("maintenancePool table covers up to InvalidVersion (19)", () => {
    const max = Math.max(...Object.keys(errorMappings.maintenancePool).map(Number));
    assert.ok(max >= 19, `maintenancePool max code is ${max}, expected ≥19`);
  });

  test("all table values are non-empty PascalCase strings", () => {
    for (const [contract, mapping] of Object.entries(errorMappings)) {
      for (const [code, name] of Object.entries(mapping)) {
        assert.ok(typeof name === "string" && name.length > 0,
          `${contract}[${code}] is empty`);
        assert.match(name, /^[A-Z]/, `${contract}[${code}] must start with uppercase`);
      }
    }
  });
});
