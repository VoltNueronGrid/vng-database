"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const node_test_1 = __importDefault(require("node:test"));
const strict_1 = __importDefault(require("node:assert/strict"));
const SecretSafeErrors_1 = require("../services/SecretSafeErrors");
(0, node_test_1.default)("redactSecrets removes admin keys, bearer tokens, and passwords", () => {
    const redacted = (0, SecretSafeErrors_1.redactSecrets)("x-vng-admin-key=super-secret authorization: Bearer top-secret password=hunter2");
    strict_1.default.equal(redacted, "x-vng-admin-key=[REDACTED] authorization: Bearer [REDACTED] password=[REDACTED]");
});
(0, node_test_1.default)("toSafeErrorMessage prefers error messages and preserves fallback semantics", () => {
    const errorMessage = (0, SecretSafeErrors_1.toSafeErrorMessage)(new Error("token=abc123"));
    const stringMessage = (0, SecretSafeErrors_1.toSafeErrorMessage)("admin api key: hidden-value");
    const fallbackMessage = (0, SecretSafeErrors_1.toSafeErrorMessage)({ reason: "opaque" }, "Request failed.");
    strict_1.default.equal(errorMessage, "token=[REDACTED]");
    strict_1.default.equal(stringMessage, "admin api key: [REDACTED]");
    strict_1.default.equal(fallbackMessage, "Request failed.");
});
//# sourceMappingURL=SecretSafeErrors.test.js.map