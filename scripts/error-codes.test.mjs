import { test, describe } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { errorMappings } from "./error-codes.mjs";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, "..");

function parseRustErrorEnum(filePath) {
  const content = fs.readFileSync(filePath, "utf8");
  const mapping = {};
  const regex = /^\s*([A-Za-z0-9_]+)\s*=\s*(\d+)\s*,/gm;
  let match;
  while ((match = regex.exec(content)) !== null) {
    const [, name, code] = match;
    mapping[parseInt(code, 10)] = name;
  }
  return mapping;
}

describe("scripts/error-codes.mjs error mapping synchronization", () => {
  test("escrow errorMappings matches contracts/escrow/src/error.rs", () => {
    const rustPath = path.join(repoRoot, "contracts", "escrow", "src", "error.rs");
    const expected = parseRustErrorEnum(rustPath);

    const actual = {};
    for (const [k, v] of Object.entries(errorMappings.escrow)) {
      actual[parseInt(k, 10)] = v;
    }

    assert.deepStrictEqual(actual, expected);
  });

  test("milestones errorMappings matches contracts/milestones/src/error.rs", () => {
    const rustPath = path.join(repoRoot, "contracts", "milestones", "src", "error.rs");
    const expected = parseRustErrorEnum(rustPath);

    const actual = {};
    for (const [k, v] of Object.entries(errorMappings.milestones)) {
      actual[parseInt(k, 10)] = v;
    }

    assert.deepStrictEqual(actual, expected);
  });

  test("maintenancePool errorMappings matches contracts/maintenance-pool/src/error.rs", () => {
    const rustPath = path.join(repoRoot, "contracts", "maintenance-pool", "src", "error.rs");
    const expected = parseRustErrorEnum(rustPath);

    const actual = {};
    for (const [k, v] of Object.entries(errorMappings.maintenancePool)) {
      actual[parseInt(k, 10)] = v;
    }

    assert.deepStrictEqual(actual, expected);
  });
});
