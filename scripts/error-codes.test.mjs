import { describe, test } from "node:test";
import assert from "node:assert/strict";

import { decodeErrorCode, formatError } from "./error-codes.mjs";

describe("contract-aware Soroban error explanations", () => {
  test("distinguishes the same code for each contract", () => {
    const cases = {
      escrow: "EscrowNotFound",
      milestones: "MilestoneNotFound",
      maintenancePool: "PoolNotFound",
    };
    for (const [contractKind, name] of Object.entries(cases)) {
      const formatted = formatError({ code: 4, message: "transaction failed" }, contractKind);
      assert.match(formatted, new RegExp(`${contractKind}: ${name}`));
      assert.doesNotMatch(formatted, /exists in multiple contracts/);
      for (const other of Object.values(cases).filter((value) => value !== name)) {
        assert.ok(!formatted.includes(other), `Unexpected ${other} in ${formatted}`);
      }
    }
  });

  test("legacy calls without context retain ambiguous interpretation", () => {
    assert.equal(decodeErrorCode(4).length, 3);
    assert.match(formatError({ code: 4 }), /exists in multiple contracts/);
  });

  test("unknown codes keep their numeric identifier without inventing meanings", () => {
    const message = formatError({ code: 999 }, "escrow");
    assert.match(message, /Detected error code: 999/);
    assert.match(message, /No known mapping for error code 999/);
  });

  test("unknown contract hints are rejected rather than decoded incorrectly", () => {
    assert.throws(() => decodeErrorCode(4, "other"), /Unknown contract kind/);
  });
});
