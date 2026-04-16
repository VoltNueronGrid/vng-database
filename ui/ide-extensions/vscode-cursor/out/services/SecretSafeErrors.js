"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.redactSecrets = redactSecrets;
exports.toSafeErrorMessage = toSafeErrorMessage;
const REDACT_PATTERNS = [
    /(x-vng-admin-key\s*[:=]\s*)([^\s,;]+)/gi,
    /(admin\s*api\s*key\s*[:=]\s*)([^\s,;]+)/gi,
    /(authorization\s*[:=]\s*bearer\s+)([^\s,;]+)/gi,
    /(token\s*[:=]\s*)([^\s,;]+)/gi,
    /(password\s*[:=]\s*)([^\s,;]+)/gi,
];
function redactSecrets(input) {
    let redacted = input;
    for (const pattern of REDACT_PATTERNS) {
        redacted = redacted.replace(pattern, (_full, prefix) => `${prefix}[REDACTED]`);
    }
    return redacted;
}
function toSafeErrorMessage(error, fallback = "Unexpected error.") {
    if (error instanceof Error && error.message.trim().length > 0) {
        return redactSecrets(error.message.trim());
    }
    if (typeof error === "string" && error.trim().length > 0) {
        return redactSecrets(error.trim());
    }
    return fallback;
}
//# sourceMappingURL=SecretSafeErrors.js.map