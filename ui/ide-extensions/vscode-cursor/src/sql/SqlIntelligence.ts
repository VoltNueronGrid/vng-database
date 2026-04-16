export interface SchemaTableRef {
  database: string;
  schema: string;
  table: string;
  fullName: string;
  columns: Set<string>;
}

export function normalizeIdentifier(value: string): string {
  return value.replace(/["`\[\]]/g, "").trim();
}

export function getActiveClause(sqlBeforeCursor: string): string {
  const upper = sqlBeforeCursor.toUpperCase();
  const clauses = ["GROUP BY", "ORDER BY", "HAVING", "SELECT", "FROM", "JOIN", "WHERE", "UPDATE", "INTO", "TABLE", "ON"];
  let activeClause = "";
  let bestIndex = -1;

  for (const clause of clauses) {
    const index = upper.lastIndexOf(clause);
    if (index > bestIndex) {
      bestIndex = index;
      activeClause = clause;
    }
  }

  return activeClause;
}

export function getCompletionContext(sqlBeforeCursor: string): "table" | "column" | "general" {
  if (/([a-zA-Z_][\w$]*)\.\s*$/.test(sqlBeforeCursor)) {
    return "column";
  }

  const activeClause = getActiveClause(sqlBeforeCursor);

  if (["FROM", "JOIN", "UPDATE", "INTO", "TABLE"].includes(activeClause)) {
    return "table";
  }

  if (["SELECT", "WHERE", "GROUP BY", "ORDER BY", "HAVING"].includes(activeClause)) {
    return "column";
  }

  return "general";
}

export function buildSortText(
  role: "snippet" | "function" | "keyword" | "table" | "column",
  activeClause: string,
  label: string
): string {
  const clausePriority: Record<string, Record<string, string>> = {
    SELECT: { column: "01", function: "02", snippet: "03", keyword: "04", table: "09" },
    WHERE: { column: "01", function: "02", keyword: "03", snippet: "04", table: "09" },
    HAVING: { column: "01", function: "02", keyword: "03", snippet: "04", table: "09" },
    "GROUP BY": { column: "01", function: "02", keyword: "03", snippet: "04", table: "09" },
    "ORDER BY": { column: "01", keyword: "02", function: "03", snippet: "04", table: "09" },
    FROM: { table: "01", snippet: "02", keyword: "03", function: "08", column: "09" },
    JOIN: { table: "01", snippet: "02", keyword: "03", function: "08", column: "09" },
    INTO: { table: "01", snippet: "02", keyword: "03", function: "08", column: "09" },
    UPDATE: { table: "01", keyword: "02", snippet: "03", function: "08", column: "09" },
    TABLE: { table: "01", keyword: "02", snippet: "03", function: "08", column: "09" },
    ON: { column: "01", keyword: "02", function: "03", snippet: "04", table: "09" },
  };

  const normalizedClause = clausePriority[activeClause] ? activeClause : "";
  const defaultPriority: Record<string, string> = { snippet: "01", function: "02", keyword: "03", table: "04", column: "05" };
  const priority = normalizedClause ? clausePriority[normalizedClause][role] ?? defaultPriority[role] : defaultPriority[role];
  return `${priority}_${label.toLowerCase()}`;
}

export function getSignatureContext(sqlBeforeCursor: string): { functionName: string; activeParameter: number } | undefined {
  let depth = 0;
  let activeParameter = 0;
  for (let index = sqlBeforeCursor.length - 1; index >= 0; index -= 1) {
    const char = sqlBeforeCursor[index];
    if (char === ")") {
      depth += 1;
      continue;
    }
    if (char === "(") {
      if (depth === 0) {
        const functionMatch = sqlBeforeCursor.slice(0, index).match(/([A-Za-z_][\w$]*)\s*$/);
        if (!functionMatch) {
          return undefined;
        }
        return {
          functionName: functionMatch[1].toUpperCase(),
          activeParameter,
        };
      }
      depth -= 1;
      continue;
    }
    if (char === "," && depth === 0) {
      activeParameter += 1;
    }
  }

  return undefined;
}

export function getAliasTarget(sqlBeforeCursor: string): string | undefined {
  const match = sqlBeforeCursor.match(/([a-zA-Z_][\w$]*)\.\s*$/);
  return match ? normalizeIdentifier(match[1]) : undefined;
}

export function extractAliases(text: string, tables: SchemaTableRef[]): Map<string, SchemaTableRef> {
  const aliases = new Map<string, SchemaTableRef>();
  const aliasRegex = /\b(from|join)\s+([a-zA-Z_][\w$]*(?:\.[a-zA-Z_][\w$]*){0,2})(?:\s+(?:as\s+)?([a-zA-Z_][\w$]*))?/gi;
  let match: RegExpExecArray | null;

  while ((match = aliasRegex.exec(text)) !== null) {
    const tableRef = normalizeIdentifier(match[2]);
    const alias = match[3] ? normalizeIdentifier(match[3]) : undefined;
    const resolvedTable = resolveTableReference(tableRef, tables);
    if (alias && resolvedTable) {
      aliases.set(alias.toLowerCase(), resolvedTable);
    }
  }

  return aliases;
}

export function resolveAliasOrTableReference(
  value: string,
  aliases: Map<string, SchemaTableRef>,
  tables: SchemaTableRef[]
): SchemaTableRef | undefined {
  const normalized = normalizeIdentifier(value).toLowerCase();
  return aliases.get(normalized) ?? resolveTableReference(value, tables);
}

export function resolveTableReference(value: string, tables: SchemaTableRef[]): SchemaTableRef | undefined {
  const normalized = normalizeIdentifier(value);
  const parts = normalized.split(".");

  if (parts.length >= 3) {
    const [database, schema, table] = parts.slice(parts.length - 3);
    return tables.find(
      (candidate) =>
        candidate.database.toLowerCase() === database.toLowerCase() &&
        candidate.schema.toLowerCase() === schema.toLowerCase() &&
        candidate.table.toLowerCase() === table.toLowerCase()
    );
  }

  if (parts.length === 2) {
    const [schema, table] = parts;
    return tables.find(
      (candidate) =>
        candidate.schema.toLowerCase() === schema.toLowerCase() && candidate.table.toLowerCase() === table.toLowerCase()
    );
  }

  return tables.find((candidate) => candidate.table.toLowerCase() === normalized.toLowerCase());
}

export function suggestNames(input: string, candidates: string[]): string[] {
  const normalizedInput = input.toLowerCase();
  const prefixMatches = candidates.filter((candidate) => candidate.toLowerCase().startsWith(normalizedInput));
  if (prefixMatches.length > 0) {
    return prefixMatches.slice(0, 3);
  }

  const fuzzyMatches = candidates
    .map((candidate) => ({
      candidate,
      score: levenshtein(normalizedInput, candidate.toLowerCase()),
    }))
    .sort((left, right) => left.score - right.score)
    .map((entry) => entry.candidate);

  return fuzzyMatches.slice(0, 3);
}

export function levenshtein(source: string, target: string): number {
  if (source === target) {
    return 0;
  }
  if (source.length === 0) {
    return target.length;
  }
  if (target.length === 0) {
    return source.length;
  }

  const matrix: number[][] = Array.from({ length: source.length + 1 }, () =>
    Array.from<number>({ length: target.length + 1 }).fill(0)
  );

  for (let i = 0; i <= source.length; i++) {
    matrix[i][0] = i;
  }

  for (let j = 0; j <= target.length; j++) {
    matrix[0][j] = j;
  }

  for (let i = 1; i <= source.length; i++) {
    for (let j = 1; j <= target.length; j++) {
      const cost = source[i - 1] === target[j - 1] ? 0 : 1;
      matrix[i][j] = Math.min(
        matrix[i - 1][j] + 1,
        matrix[i][j - 1] + 1,
        matrix[i - 1][j - 1] + cost
      );
    }
  }

  return matrix[source.length][target.length];
}

export function getSuggestionsFromDiagnosticMessage(message: string): string[] {
  const marker = " Suggestions: ";
  const markerIndex = message.indexOf(marker);
  if (markerIndex === -1) {
    return [];
  }

  return message
    .slice(markerIndex + marker.length)
    .split(",")
    .map((item) => item.trim())
    .filter((item) => item.length > 0);
}