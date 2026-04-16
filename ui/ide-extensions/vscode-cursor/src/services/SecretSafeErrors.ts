const REDACT_PATTERNS: RegExp[] = [
  /(x-vng-admin-key\s*[:=]\s*)([^\s,;]+)/gi,
  /(admin\s*api\s*key\s*[:=]\s*)([^\s,;]+)/gi,
  /(authorization\s*[:=]\s*bearer\s+)([^\s,;]+)/gi,
  /(token\s*[:=]\s*)([^\s,;]+)/gi,
  /(password\s*[:=]\s*)([^\s,;]+)/gi,
];

export function redactSecrets(input: string): string {
  let redacted = input;

  for (const pattern of REDACT_PATTERNS) {
    redacted = redacted.replace(pattern, (_full, prefix: string) => `${prefix}[REDACTED]`);
  }

  return redacted;
}

export function toSafeErrorMessage(error: unknown, fallback = "Unexpected error."): string {
  if (error instanceof Error && error.message.trim().length > 0) {
    return redactSecrets(error.message.trim());
  }

  if (typeof error === "string" && error.trim().length > 0) {
    return redactSecrets(error.trim());
  }

  return fallback;
}
