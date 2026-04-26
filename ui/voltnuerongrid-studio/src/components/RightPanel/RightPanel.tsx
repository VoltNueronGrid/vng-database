import { useUiStore } from "@/store/ui";
import { useConnectionStore } from "@/store/connection";
import { useEditorStore } from "@/store/editor";

function colTypeClass(type: string): string {
  const t = type.toUpperCase();
  if (t.includes("INT") || t.includes("FLOAT") || t.includes("DECIMAL") || t.includes("DOUBLE"))
    return "int";
  if (t.includes("BOOL")) return "bool";
  if (t.includes("DATE") || t.includes("TIME")) return "date";
  if (t.includes("JSON")) return "json";
  return "str";
}

export function RightPanel() {
  const closeRightPanel = useUiStore((s) => s.closeRightPanel);
  const rightPanelTable = useUiStore((s) => s.rightPanelTable); // "schema.table"
  const databases = useConnectionStore((s) => s.getDatabases());
  const openTableTab = useEditorStore((s) => s.openTableTab);

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
                <button className="btn" style={{ justifyContent: "center", fontSize: 11.5 }}>
                  View DDL
                </button>
                <button className="btn" style={{ justifyContent: "center", fontSize: 11.5 }}>
                  Analyze Table
                </button>
                <button
                  className="btn danger"
                  style={{ justifyContent: "center", fontSize: 11.5 }}
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
