import { test, describe } from "node:test";
import assert from "node:assert/strict";
import {
  errorMappings,
  extractErrorCode,
  decodeErrorCode,
  formatError,
} from "./error-codes.mjs";

describe("extractErrorCode pattern tightening (#860)", () => {
  test("extracts error code when explicit separator is present", () => {
    assert.strictEqual(extractErrorCode("Transaction failed with error code: 7"), 7);
    assert.strictEqual(extractErrorCode("error code=7"), 7);
    assert.strictEqual(extractErrorCode("HostError: code: 12"), 12);
    assert.strictEqual(extractErrorCode("HostError: code=12"), 12);
  });

  test("rejects free-text error strings like 'error 500' without explicit separator", () => {
    assert.strictEqual(extractErrorCode("HTTP error 500: Internal Server Error"), null);
    assert.strictEqual(extractErrorCode("proxy error 502 Bad Gateway"), null);
    assert.strictEqual(extractErrorCode("error 404"), null);
  });

  test("rejects arbitrary free-text strings matching 'value N' or 'value: N'", () => {
    assert.strictEqual(extractErrorCode("Unexpected value 42 in stream"), null);
    assert.strictEqual(extractErrorCode("Maximum allowed value: 100 exceeded"), null);
  });

  test("extracts error code from Send failed / Tx failed string with embedded JSON", () => {
    const sendFailedStr = 'Send failed: {"errorResult": {"code": 14}}';
    assert.strictEqual(extractErrorCode(sendFailedStr), 14);

    const txFailedStr = 'Tx failed: {"code": 8}';
    assert.strictEqual(extractErrorCode(txFailedStr), 8);
  });

  test("extracts error code from structured error objects", () => {
    assert.strictEqual(extractErrorCode({ code: 5 }), 5);
    assert.strictEqual(extractErrorCode({ error: 6 }), 6);
    assert.strictEqual(extractErrorCode({ value: 3 }), 3);
    assert.strictEqual(extractErrorCode({ errorResult: { code: 9 } }), 9);
  });

  test("returns null for empty or non-error values", () => {
    assert.strictEqual(extractErrorCode(null), null);
    assert.strictEqual(extractErrorCode(undefined), null);
    assert.strictEqual(extractErrorCode(""), null);
    assert.strictEqual(extractErrorCode("A general transport failure occurred"), null);
  });
});

describe("decodeErrorCode and formatError integration tests", () => {
  test("decodes known error codes properly", () => {
    const meanings = decodeErrorCode(1);
    assert.ok(meanings.includes("escrow: AlreadyInitialized"));
    assert.ok(meanings.includes("milestones: AlreadyInitialized"));
    assert.ok(meanings.includes("maintenancePool: AlreadyInitialized"));
  });

  test("formatError handles unstructured HTTP errors without false diagnosis (#860)", () => {
    const formatted = formatError("HTTP error 500: Bad Gateway");
    assert.match(formatted, /No error code detected/);
    assert.doesNotMatch(formatted, /Detected error code/);
  });

  test("formatError formats valid error codes with detected meanings", () => {
    const formatted = formatError("failed with error code: 3");
    assert.match(formatted, /Detected error code: 3/);
    assert.match(formatted, /escrow: Unauthorized/);
  });
});
