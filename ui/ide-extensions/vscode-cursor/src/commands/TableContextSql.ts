import { Column, Table } from "../models/Schema";

export function generateUpdateTemplate(table: Table): string {
  const mutableColumns = table.columns
    .filter((column) => !column.isPrimaryKey)
    .map((column) => `"${column.name}" = ?`);

  if (mutableColumns.length === 0) {
    return `-- No mutable columns found on "${table.schema}"."${table.name}"\n-- Table appears to contain only primary key columns.`;
  }

  const sets = mutableColumns.join(",\n  ");
  const pk = table.columns.find((column) => column.isPrimaryKey);
  const whereClause = pk ? `WHERE "${pk.name}" = ?;` : "WHERE 1=1;";
  return `UPDATE "${table.schema}"."${table.name}"\nSET ${sets}\n${whereClause}`;
}

export function generateDeleteTemplate(table: Table): string {
  const pk = table.columns.find((column) => column.isPrimaryKey);
  const whereClause = pk ? `WHERE "${pk.name}" = ?;` : "WHERE 1=1;";
  return `DELETE FROM "${table.schema}"."${table.name}"\n${whereClause}`;
}

export function generateTruncateTableSql(table: Table): string {
  return `TRUNCATE TABLE "${table.schema}"."${table.name}";`;
}

export function generateMockData(table: Table, rowCount: number = 5): string {
  const mockValueGetter = (column: Column): string => {
    const type = column.type.toUpperCase();

    if (type.includes("INT") || type.includes("BIGINT")) {
      return Math.floor(Math.random() * 10000).toString();
    }
    if (type.includes("FLOAT") || type.includes("DOUBLE") || type.includes("DECIMAL")) {
      return (Math.random() * 100).toFixed(2);
    }
    if (type === "BOOLEAN" || type.includes("BOOL")) {
      return Math.random() > 0.5 ? "true" : "false";
    }
    if (type.includes("DATE")) {
      return `'${new Date(Date.now() - Math.random() * 365 * 24 * 60 * 60 * 1000).toISOString().split("T")[0]}'`;
    }
    if (type.includes("TIMESTAMP")) {
      return `'${new Date().toISOString()}'`;
    }
    if (type.includes("VARCHAR") || type === "TEXT") {
      return `'Sample Data ${Math.random().toString(36).substring(7)}'`;
    }
    return "NULL";
  };

  const inserts: string[] = [];
  const columns = table.columns.map((column) => `"${column.name}"`).join(", ");

  for (let index = 0; index < rowCount; index++) {
    const values = table.columns.map((column) => mockValueGetter(column)).join(", ");
    inserts.push(`INSERT INTO "${table.schema}"."${table.name}" (${columns}) VALUES (${values});`);
  }

  return inserts.join("\n");
}
