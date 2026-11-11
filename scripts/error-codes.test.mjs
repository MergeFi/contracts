import { test, describe } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  errorMappings,
  extractErrorCode,
  decodeErrorCode,
  formatError,
} from "./error-codes.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const rootDir = path.resolve(__dirname, "..");

describe("errorMappings variant coverage (#857)", () => {
  test("escrow includes ArithmeticOverflow (21), InvalidVersion (22), TooManyRecipients (24)", () => {
    assert.strictEqual(errorMappings.escrow[21], "ArithmeticOverflow");
    assert.strictEqual(errorMappings.escrow[22], "InvalidVersion");
    assert.strictEqual(errorMappings.escrow[24], "TooManyRecipients");
  });

  test("milestones includes TooManyRecipients (25)", () => {
    assert.strictEqual(errorMappings.milestones[25], "TooManyRecipients");
  });

  test("maintenancePool includes TooManyRecipients (20)", () => {
    assert.strictEqual(errorMappings.maintenancePool[20], "TooManyRecipients");
  });

  test("decodeErrorCode returns proper meanings for the new error codes", () => {
    const meanings24 = decodeErrorCode(24);
    assert.ok(meanings24.includes("escrow: TooManyRecipients"));

    const meanings25 = decodeErrorCode(25);
    assert.ok(meanings25.includes("milestones: TooManyRecipients"));

    const meanings20 = decodeErrorCode(20);
    assert.ok(meanings20.includes("maintenancePool: TooManyRecipients"));

    const meanings21 = decodeErrorCode(21);
    assert.ok(meanings21.includes("escrow: ArithmeticOverflow"));
    assert.ok(meanings21.includes("milestones: DeadlinePassed"));

    const meanings22 = decodeErrorCode(22);
    assert.ok(meanings22.includes("escrow: InvalidVersion"));
    assert.ok(meanings22.includes("milestones: DeadlineInPast"));
  });

  test("formatError does not output 'No known mapping' for the new codes", () => {
    const formattedEscrow = formatError("Transaction failed: code 24");
    assert.match(formattedEscrow, /escrow: TooManyRecipients/);
    assert.doesNotMatch(formattedEscrow, /No known mapping/);

    const formattedMilestones = formatError("Transaction failed: code 25");
    assert.match(formattedMilestones, /milestones: TooManyRecipients/);
    assert.doesNotMatch(formattedMilestones, /No known mapping/);

    const formattedMaintenance = formatError("Transaction failed: code 20");
    assert.match(formattedMaintenance, /maintenancePool: TooManyRecipients/);
    assert.doesNotMatch(formattedMaintenance, /No known mapping/);
  });
});

describe("contract error.rs and error-codes.mjs parity assertion (#857)", () => {
  /**
   * Helper function to parse enum variants and values from Rust error.rs
   */
  function parseRustErrors(filePath) {
    const content = fs.readFileSync(filePath, "utf-8");
    const enumMatch = content.match(/pub\s+enum\s+Error\s*\{([\s\S]*?)\}/);
    if (!enumMatch) {
      throw new Error(`Could not find 'pub enum Error' in ${filePath}`);
    }

    const variants = {};
    const lines = enumMatch[1].split("\n");
    for (const line of lines) {
      const match = line.match(/^\s*([A-Za-z0-9_]+)\s*=\s*(\d+)/);
      if (match) {
        variants[parseInt(match[2], 10)] = match[1];
      }
    }
    return variants;
  }

  test("escrow errorMappings strictly matches contracts/escrow/src/error.rs", () => {
    const errorRsPath = path.join(rootDir, "contracts", "escrow", "src", "error.rs");
    const rustVariants = parseRustErrors(errorRsPath);

    for (const [codeStr, name] of Object.entries(rustVariants)) {
      const code = parseInt(codeStr, 10);
      assert.strictEqual(
        errorMappings.escrow[code],
        name,
        `Missing or mismatched escrow error variant for code ${code} (expected ${name})`
      );
    }
  });

  test("milestones errorMappings strictly matches contracts/milestones/src/error.rs", () => {
    const errorRsPath = path.join(rootDir, "contracts", "milestones", "src", "error.rs");
    const rustVariants = parseRustErrors(errorRsPath);

    for (const [codeStr, name] of Object.entries(rustVariants)) {
      const code = parseInt(codeStr, 10);
      assert.strictEqual(
        errorMappings.milestones[code],
        name,
        `Missing or mismatched milestones error variant for code ${code} (expected ${name})`
      );
    }
  });

  test("maintenancePool errorMappings strictly matches contracts/maintenance-pool/src/error.rs", () => {
    const errorRsPath = path.join(rootDir, "contracts", "maintenance-pool", "src", "error.rs");
    const rustVariants = parseRustErrors(errorRsPath);

    for (const [codeStr, name] of Object.entries(rustVariants)) {
      const code = parseInt(codeStr, 10);
      assert.strictEqual(
        errorMappings.maintenancePool[code],
        name,
        `Missing or mismatched maintenancePool error variant for code ${code} (expected ${name})`
      );
    }
  });
});

describe("extractErrorCode & formatError general unit tests", () => {
  test("returns null for non-error or empty inputs", () => {
    assert.strictEqual(extractErrorCode(null), null);
    assert.strictEqual(extractErrorCode(undefined), null);
    assert.strictEqual(extractErrorCode(""), null);
  });

  test("extracts error code from JSON and object structures", () => {
    const errorStr = 'Send failed: {"errorResult": {"code": 14}}';
    assert.strictEqual(extractErrorCode(errorStr), 14);
    assert.strictEqual(extractErrorCode({ code: 5 }), 5);
    assert.strictEqual(extractErrorCode({ errorResult: { code: 9 } }), 9);
  });

  test("extracts error code from string regex patterns", () => {
    assert.strictEqual(extractErrorCode("Transaction failed with error code: 7"), 7);
    assert.strictEqual(extractErrorCode("HostError: code 12"), 12);
  });

  test("formatError formats unmapped and unknown codes gracefully", () => {
    const formatted = formatError("Unknown network issue");
    assert.match(formatted, /No error code detected/);

    const unmapped = formatError("Failed with code: 9999");
    assert.match(unmapped, /No known mapping for error code 9999/);
  });
});
