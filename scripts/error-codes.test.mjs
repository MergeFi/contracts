import { test, describe } from "node:test";
import assert from "node:assert/strict";
import {
  errorMappings,
  extractErrorCode,
  decodeErrorCode,
  formatError,
  normalizeContractName,
} from "./error-codes.mjs";

describe("normalizeContractName unit tests (#856)", () => {
  test("returns null when name is null or undefined", () => {
    assert.strictEqual(normalizeContractName(null), null);
    assert.strictEqual(normalizeContractName(undefined), null);
    assert.strictEqual(normalizeContractName(""), null);
  });

  test("normalizes maintenance-pool variants to maintenancePool", () => {
    assert.strictEqual(normalizeContractName("maintenance-pool"), "maintenancePool");
    assert.strictEqual(normalizeContractName("maintenance_pool"), "maintenancePool");
    assert.strictEqual(normalizeContractName("maintenancePool"), "maintenancePool");
    assert.strictEqual(normalizeContractName("MAINTENANCE-POOL"), "maintenancePool");
  });

  test("normalizes escrow and milestones names", () => {
    assert.strictEqual(normalizeContractName("escrow"), "escrow");
    assert.strictEqual(normalizeContractName("ESCROW"), "escrow");
    assert.strictEqual(normalizeContractName("milestones"), "milestones");
    assert.strictEqual(normalizeContractName("Milestones"), "milestones");
  });

  test("throws descriptive error when contract name is unrecognized", () => {
    assert.throws(
      () => normalizeContractName("unknown-contract"),
      /Unknown contract name: 'unknown-contract'\. Valid contracts are: escrow, milestones, maintenancePool/
    );
  });
});

describe("extractErrorCode unit tests (#855)", () => {
  test("returns null for empty or non-error values", () => {
    assert.strictEqual(extractErrorCode(null), null);
    assert.strictEqual(extractErrorCode(undefined), null);
    assert.strictEqual(extractErrorCode(""), null);
  });

  test("extracts error code from Send failed / Tx failed string with embedded JSON", () => {
    const errorStr = 'Send failed: {"errorResult": {"code": 14}}';
    assert.strictEqual(extractErrorCode(errorStr), 14);
  });

  test("extracts error code from free-text regex patterns", () => {
    assert.strictEqual(extractErrorCode("Transaction failed with error code: 7"), 7);
    assert.strictEqual(extractErrorCode("HostError: code 12"), 12);
  });

  test("extracts error code from object structures", () => {
    assert.strictEqual(extractErrorCode({ code: 5 }), 5);
    assert.strictEqual(extractErrorCode({ value: 3 }), 3);
    assert.strictEqual(extractErrorCode({ errorResult: { code: 9 } }), 9);
    assert.strictEqual(extractErrorCode(new Error("HostError: code 6")), 6);
  });
});

describe("decodeErrorCode unit tests (#855)", () => {
  test("returns empty array for null or undefined", () => {
    assert.deepStrictEqual(decodeErrorCode(null), []);
    assert.deepStrictEqual(decodeErrorCode(undefined), []);
  });

  test("decodes code present across multiple contracts", () => {
    const meanings = decodeErrorCode(1);
    assert.ok(meanings.includes("escrow: AlreadyInitialized"));
    assert.ok(meanings.includes("milestones: AlreadyInitialized"));
    assert.ok(meanings.includes("maintenancePool: AlreadyInitialized"));
  });

  test("decodes code specific to a single contract", () => {
    const meanings = decodeErrorCode(3);
    assert.ok(meanings.includes("escrow: Unauthorized"));
    assert.ok(meanings.includes("milestones: MilestoneNotFound"));
    assert.ok(meanings.includes("maintenancePool: PoolNotFound"));
  });

  test("returns empty array for unmapped error code", () => {
    assert.deepStrictEqual(decodeErrorCode(9999), []);
  });
});

describe("formatError unit tests (#856, #855)", () => {
  test("formats error with decoded meanings across all contracts when no contract specified", () => {
    const formatted = formatError("failed with code: 1");
    assert.match(formatted, /Detected error code: 1/);
    assert.match(formatted, /escrow: AlreadyInitialized/);
    assert.match(formatted, /milestones: AlreadyInitialized/);
    assert.match(formatted, /maintenancePool: AlreadyInitialized/);
    assert.match(formatted, /Error code 1 exists in multiple contracts/);
  });

  test("narrows error meanings when CONTRACT_NAME is maintenance-pool (#856)", () => {
    const formatted = formatError("failed with error code: 3", "maintenance-pool");
    assert.match(formatted, /Detected error code: 3/);
    assert.match(formatted, /maintenancePool: PoolNotFound/);
    assert.doesNotMatch(formatted, /escrow: Unauthorized/);
    assert.doesNotMatch(formatted, /milestones: MilestoneNotFound/);
  });

  test("narrows error meanings when CONTRACT_NAME is escrow", () => {
    const formatted = formatError("failed with error code: 3", "escrow");
    assert.match(formatted, /Detected error code: 3/);
    assert.match(formatted, /escrow: Unauthorized/);
    assert.doesNotMatch(formatted, /milestones: MilestoneNotFound/);
    assert.doesNotMatch(formatted, /maintenancePool: PoolNotFound/);
  });

  test("handles errors with no detected code gracefully", () => {
    const formatted = formatError("Connection timed out");
    assert.match(formatted, /No error code detected/);
  });

  test("throws error when invalid contract name is passed", () => {
    assert.throws(() => formatError("failed with code: 1", "invalid-contract"), /Unknown contract name/);
  });
});
