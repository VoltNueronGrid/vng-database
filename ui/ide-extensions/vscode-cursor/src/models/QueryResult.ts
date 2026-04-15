/**
 * Query execution and result models
 */

export interface QueryResult {
  id: string; // unique result ID
  query: string; // the SQL that was executed
  status: "pending" | "success" | "error" | "cancelled";
  rows: Record<string, any>[];
  columns: QueryColumn[];
  rowCount: number;
  affectedRows?: number;
  executionTime: number; // milliseconds
  timestamp: number;
  error?: {
    message: string;
    code?: string;
    detail?: string;
  };
}

export interface QueryColumn {
  name: string;
  type: string;
  index: number;
}

export interface QueryHistoryEntry {
  id: string;
  query: string;
  connectionId: string;
  timestamp: number;
  executionTime?: number;
  status: "success" | "error" | "cancelled";
  resultId?: string; // reference to QueryResult
}

/**
 * Parse query result from server response
 */
export function parseQueryResult(
  query: string,
  data: any,
  executionTime: number
): QueryResult {
  const id = `result-${Date.now()}`;
  const timestamp = Date.now();

  // Handle different response formats
  let rows: Record<string, any>[] = [];
  let columns: QueryColumn[] = [];

  if (Array.isArray(data)) {
    // Response is array of rows
    rows = data;
    // Infer columns from first row
    if (rows.length > 0) {
      const firstRow = rows[0];
      columns = Object.keys(firstRow).map((name, index) => ({
        name,
        type: typeof firstRow[name],
        index,
      }));
    }
  } else if (data && typeof data === "object") {
    // Response is object with data/rows and possibly columns
    rows = data.rows || data.data || [];
    columns = data.columns || [];
    if (columns.length === 0 && rows.length > 0) {
      const firstRow = rows[0];
      columns = Object.keys(firstRow).map((name, index) => ({
        name,
        type: typeof firstRow[name],
        index,
      }));
    }
  }

  return {
    id,
    query,
    status: "success",
    rows,
    columns,
    rowCount: rows.length,
    executionTime,
    timestamp,
  };
}

/**
 * Export result as CSV
 */
export function exportAsCSV(result: QueryResult): string {
  if (result.columns.length === 0) {
    return "";
  }

  // CSV header
  const header = result.columns.map((col) => `"${col.name.replace(/"/g, '""')}"`).join(",");

  // CSV rows
  const csvRows = result.rows.map((row) =>
    result.columns
      .map((col) => {
        const value = row[col.name];
        if (value === null || value === undefined) {
          return "";
        }
        const strValue = String(value).replace(/"/g, '""');
        return `"${strValue}"`;
      })
      .join(",")
  );

  return [header, ...csvRows].join("\n");
}

/**
 * Export result as JSON
 */
export function exportAsJSON(result: QueryResult): string {
  return JSON.stringify(result.rows, null, 2);
}

/**
 * Export result as INSERT statements
 */
export function exportAsInsertSQL(result: QueryResult, tableName: string): string {
  if (result.columns.length === 0 || result.rows.length === 0) {
    return "";
  }

  const columnNames = result.columns.map((col) => col.name).join(", ");
  const inserts = result.rows.map((row) => {
    const values = result.columns
      .map((col) => {
        const value = row[col.name];
        if (value === null || value === undefined) {
          return "NULL";
        }
        if (typeof value === "string") {
          return `'${value.replace(/'/g, "''")}'`;
        }
        if (typeof value === "boolean") {
          return value ? "true" : "false";
        }
        return String(value);
      })
      .join(", ");
    return `INSERT INTO ${tableName} (${columnNames}) VALUES (${values});`;
  });

  return inserts.join("\n");
}
