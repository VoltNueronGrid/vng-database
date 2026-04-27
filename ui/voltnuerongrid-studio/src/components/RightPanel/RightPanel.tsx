import { useUiStore } from "@/store/ui";
import { useConnectionStore } from "@/store/connection";
import { useEditorStore } from "@/store/editor";
import type { SchemaColumn } from "@/api/studio-client";

function colTypeClass(type: string): string {
  const t = type.toUpperCase();
  if (t.includes("INT") || t.includes("FLOAT") || t.includes("DECIMAL") || t.includes("DOUBLE"))
    return "int";
  if (t.includes("BOOL")) return "bool";
  if (t.includes("DATE") || t.includes("TIME")) return "date";
  if (t.includes("JSON")) return "json";
  return "str";
}

/** Return a type-appropriate SQL literal for INSERT / DDL templates. */
function typedDefault(col: SchemaColumn): string {
  const t = col.data_type.toUpperCase();
  if (t.includes("BOOL"))                                        return "true";
  if (t.includes("INT") || t.includes("SERIAL"))                return "1";
  if (t.includes("FLOAT") || t.includes("DOUBLE") ||
      t.includes("DECIMAL") || t.includes("NUMERIC"))           return "0.00";
  if (t.includes("TIMESTAMP") || t.includes("DATETIME"))        return "CURRENT_TIMESTAMP";
  if (t.includes("DATE"))                                        return "CURRENT_DATE";
  if (t.includes("TIME"))                                        return "CURRENT_TIME";
  if (t.includes("JSON"))                                        return "'{}'";
  if (t.includes("UUID"))                                        return "gen_random_uuid()";
  return "'value'";
}

export function RightPanel() {
  const closeRightPanel = useUiStore((s) => s.closeRightPanel);
  const rightPanelTable = useUiStore((s) => s.rightPanelTable); // "schema.table"
  const databases = useConnectionStore((s) => s.getDatabases());
  const openTableTab = useEditorStore((s) => s.openTableTab);
  const openSqlTab = useEditorStore((s) => s.openSqlTab);

  // Resolve the table from schema
  const parts = rightPanelTable?.split(".") ?? [];
  const schemaName = parts[0];
  const tableName = parts[1];

  let tableInfo = null;
  for (const db of databases) {
    for (const ns of db.schemas) {
      if (ns.name === schemaName) {
        const t = ns.tables.find((t) => t.name === tableName);
        if (t) { tableInfo = t; break; }
      }
    }
  }

  function handleGenerateInsert() {
    if (!tableInfo) return;
    const cols = tableInfo.columns.map((c) => c.name).join(", ");
    const vals = tableInfo.columns.map((c) => typedDefault(c)).join(", ");
    openSqlTab(
      `INSERT INTO ${tableInfo.schema}.${tableInfo.name} (${cols})\nVALUES (${vals});`,
      `insert_${tableInfo.name}.sql`
    );
  }

  function handleViewDDL() {
    if (!tableInfo) return;
    const cols = tableInfo.columns.map((c) => {
      const pk = c.primary_key ? " PRIMARY KEY" : "";
      const nn = !c.nullable && !c.primary_key ? " NOT NULL" : "";
      return `  ${c.name} ${c.data_type}${pk}${nn}`;
    }).join(",\n");
    openSqlTab(
      `-- Reconstructed DDL for ${tableInfo.schema}.${tableInfo.name}\nCREATE TABLE ${tableInfo.schema}.${tableInfo.name} (\n${cols}\n);`,
      `ddl_${tableInfo.name}.sql`
    );
  }

  function handleAnalyze() {
    if (!tableInfo) return;
    openSqlTab(
      `SELECT COUNT(*) AS total_rows FROM ${tableInfo.schema}.${tableInfo.name};`,
      `analyze_${tableInfo.name}.sql`
    );
  }

  function handleTruncate() {
    if (!tableInfo) return;
    if (!confirm(`Truncate table ${tableInfo.schema}.${tableInfo.name}? This cannot be undone.`)) return;
    openSqlTab(
      `TRUNCATE TABLE ${tableInfo.schema}.${tableInfo.name};`,
      `truncate_${tableInfo.name}.sql`
    );
  }

  return (
    <div className="right-panel">
      <div className="panel-header">
        <span className="tree-icon">📋</span>
        {tableName ?? "Table"}
        <button className="panel-close" onClick={closeRightPanel}>✕</button>
      </div>
      <div className="panel-body">
        {tableInfo ? (
          <>
            <div className="detail-section">
              <div className="detail-title">Stats</div>
              <div className="detail-stat-grid">
                <div className="detail-stat">
                  <div className="ds-val cyan">
                    {tableInfo.row_count != null
                      ? tableInfo.row_count.toLocaleString()
                      : "—"}
                  </div>
                  <div className="ds-label">Rows</div>
                </div>
                <div className="detail-stat">
                  <div className="ds-val">{tableInfo.columns.length}</div>
                  <div className="ds-label">Columns</div>
                </div>
              </div>
            </div>

            <div className="detail-section">
              <div className="detail-title">Columns</div>
              <div className="col-list">
                {tableInfo.columns.map((col) => (
                  <div key={col.name} className="col-row">
                    {col.primary_key && (
                      <span className="pk-marker" title="Primary key">🔑</span>
                    )}
                    {!col.primary_key && (
                      <span style={{ width: 14 }} />
                    )}
                    <span className="col-row-name mono">{col.name}</span>
                    <span className={`col-chip ${colTypeClass(col.data_type)}`}>
                      {col.data_type}
                    </span>
                  </div>
                ))}
              </div>
            </div>

            <div className="detail-section">
              <div className="detail-title">Quick Actions</div>
              <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
                <button
                  className="btn"
                  style={{ justifyContent: "center", fontSize: 11.5 }}
                  onClick={() => tableInfo && openTableTab(tableInfo.name, tableInfo.schema)}
                >
                  SELECT * LIMIT 100
                </button>
                <button
                  className="btn"
                  style={{ justifyContent: "center", fontSize: 11.5 }}
                  onClick={handleGenerateInsert}
                >
                  Generate INSERT
                </button>
                <button
                  className="btn"
                  style={{ justifyContent: "center", fontSize: 11.5 }}
                  onClick={handleViewDDL}
                >
                  View DDL
                </button>
                <button
                  className="btn"
                  style={{ justifyContent: "center", fontSize: 11.5 }}
                  onClick={handleAnalyze}
                >
                  Analyze Table
                </button>
                <button
                  className="btn danger"
                  style={{ justifyContent: "center", fontSize: 11.5 }}
                  onClick={handleTruncate}
                >
                  TRUNCATE TABLE
                </button>
              </div>
            </div>
          </>
        ) : (
          <div className="results-empty">
            <div className="re-icon">📋</div>
            <div className="text-muted">
              {rightPanelTable ? "Table not found in schema" : "Select a table"}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
