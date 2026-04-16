import test from "node:test";
import assert from "node:assert/strict";
import { redactSecrets, toSafeErrorMessage } from "../services/SecretSafeErrors";

test("redactSecrets removes admin keys, bearer tokens, and passwords", () => {
  const redacted = redactSecrets(
    "x-vng-admin-key=super-secret authorization: Bearer top-secret password=hunter2"
  );

  assert.equal(
    redacted,
    "x-vng-admin-key=[REDACTED] authorization: Bearer [REDACTED] password=[REDACTED]"
  );
});

test("toSafeErrorMessage prefers error messages and preserves fallback semantics", () => {
  const errorMessage = toSafeErrorMessage(new Error("token=abc123"));
  const stringMessage = toSafeErrorMessage("admin api key: hidden-value");
  const fallbackMessage = toSafeErrorMessage({ reason: "opaque" }, "Request failed.");

  assert.equal(errorMessage, "token=[REDACTED]");
  assert.equal(stringMessage, "admin api key: [REDACTED]");
  assert.equal(fallbackMessage, "Request failed.");
});