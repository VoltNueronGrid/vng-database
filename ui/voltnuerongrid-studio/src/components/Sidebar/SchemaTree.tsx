import { useState } from "react";
import { useConnectionStore } from "@/store/connection";
import { useUiStore } from "@/store/ui";
import { useEditorStore } from "@/store/editor";
import { openMenuFor } from "@/store/contextMenu";
import {
  buildDatabaseMenu,
  buildSchemaMenu,
  buildTableMenu,
  buildColumnMenu,
} from "@/components/ContextMenu/menus";
import type { SchemaDatabase, SchemaNamespace, SchemaTable, SchemaFunction } from "@/api/studio-client";

function colTypeClass(type: string): string {
  const t = type.toUpperCase();
  if (t.includes("INT") || t.includes("FLOAT") || t.includes("DECIMAL") || t.includes("NUM"))
    return "int";
  if (t.includes("BOOL")) return "bool";
  if (t.includes("DATE") || t.includes("TIME")) return "date";
  if (t.includes("JSON")) return "json";
  return "str";
}

function TableNode({ table, schemaName, dbName }: { table: SchemaTable; schemaName: string; dbName: string }) {
  const [open, setOpen] = useState(false);
  const openRightPanel = useUiStore((s) => s.openRightPanel);
  const openTableTab = useEditorStore((s) => s.openTableTab);

  // Server may return table.name as qualified ("schema.table" or "db.schema.table").
  // Strip qualification so all UI operations use the short name.
  const tableBaseName = table.name.split(".").pop() ?? table.name;

  return (
    <>
      <div
        className="tree-node"
        style={{ paddingLeft: 0 }}
        onClick={() => setOpen((o) => !o)}
        onDoubleClick={() => openTableTab(tableBaseName, schemaName)}
        onContextMenu={openMenuFor(() => buildTableMenu(dbName, schemaName, table))}
      >
        <span className="tree-indent" />
        <span className="tree-indent" />
        <span className="tree-indent" />
        <span className={`tree-chevron ${open ? "open" : ""}`}>▶</span>
        <span className="tree-icon">📋</span>
        <span className="tree-label">{tableBaseName}</span>
        {table.row_count != null && (
          <span className="tree-count">
            {table.row_count >= 1_000_000
              ? `${(table.row_count / 1_000_000).toFixed(1)}M`
              : table.row_count >= 1_000
              ? `${(table.row_count / 1_000).toFixed(0)}K`
              : table.row_count}
          </span>
        )}
      </div>
      {open && table.columns.map((col) => (
        <div
          key={col.name}
          className="tree-node"
          style={{ paddingLeft: 0 }}
          onClick={() => openRightPanel(`${schemaName}.${tableBaseName}`)}
          onContextMenu={openMenuFor(() => buildColumnMenu(dbName, schemaName, tableBaseName, col))}
        >
          <span className="tree-indent" />
          <span className="tree-indent" />
          <span className="tree-indent" />
          <span className="tree-indent" />
          <span className="tree-chevron" style={{ visibility: "hidden" }}>▶</span>
          {col.primary_key
            ? <span className="pk-marker" title="Primary key">🔑</span>
            : <span style={{ width: 14 }} />
          }
          <span className="tree-label mono" style={{ fontSize: 11 }}>{col.name}</span>
          <span className={`col-chip ${colTypeClass(col.data_type)}`}>{col.data_type}</span>
        </div>
      ))}
    </>
  );
}

function FunctionNode({ fn: func, schemaName }: { fn: SchemaFunction; schemaName: string }) {
  const [open, setOpen] = useState(false);
  const insertText = useEditorStore((s) => s.insertTextIntoActiveTab);

  const callSnippet = `CALL ${func.name}(${
    func.arguments
      ? func.arguments.split(",").map((_arg, i) => `$${i + 1}`).join(", ")
      : ""
  })`;

  return (
    <>
      <div
        className="tree-node"
        style={{ paddingLeft: 0 }}
        onClick={() => setOpen((o) => !o)}
        onDoubleClick={() => insertText?.(callSnippet)}
        title={`Double-click to insert CALL snippet\n\n${func.definition}`}
      >
        <span className="tree-indent" />
        <span className="tree-indent" />
        <span className="tree-indent" />
        <span className={`tree-chevron ${open ? "open" : ""}`}>▶</span>
        <span className="tree-icon">⚡</span>
        <span className="tree-label">{func.name}</span>
        <span className="col-chip" style={{ background: "var(--accent-1, #7c3aed22)", color: "var(--accent, #7c3aed)", fontSize: 10, padding: "1px 5px", borderRadius: 4, marginLeft: 4 }}>
          fn
        </span>
      </div>
      {open && (
        <>
          {/* Arguments */}
          <div className="tree-node" style={{ paddingLeft: 0 }}>
            <span className="tree-indent" />
            <span className="tree-indent" />
            <span className="tree-indent" />
            <span className="tree-indent" />
            <span className="tree-chevron" style={{ visibility: "hidden" }}>▶</span>
            <span style={{ width: 14 }} />
            <span className="tree-label mono" style={{ fontSize: 11, color: "var(--text-2)" }}>({func.arguments || "void"})</span>
            <span className="col-chip str" style={{ marginLeft: 4 }}>args</span>
          </div>
          {/* Return type */}
          <div className="tree-node" style={{ paddingLeft: 0 }}>
            <span className="tree-indent" />
            <span className="tree-indent" />
            <span className="tree-indent" />
            <span className="tree-indent" />
            <span className="tree-chevron" style={{ visibility: "hidden" }}>▶</span>
            <span style={{ width: 14 }} />
            <span className="tree-label mono" style={{ fontSize: 11, color: "var(--text-2)" }}>→ {func.return_type}</span>
            <span className="col-chip str" style={{ marginLeft: 4 }}>returns</span>
          </div>
          {/* Language */}
          <div className="tree-node" style={{ paddingLeft: 0 }}>
            <span className="tree-indent" />
            <span className="tree-indent" />
            <span className="tree-indent" />
            <span className="tree-indent" />
            <span className="tree-chevron" style={{ visibility: "hidden" }}>▶</span>
            <span style={{ width: 14 }} />
            <span className="tree-label mono" style={{ fontSize: 11, color: "var(--text-2)" }}>{func.language}</span>
            <span className="col-chip str" style={{ marginLeft: 4 }}>lang</span>
          </div>
        </>
      )}
    </>
  );
}

function SchemaNode({ ns, dbName }: { ns: SchemaNamespace; dbName: string }) {
  const [open, setOpen] = useState(true);
  const functions = ns.functions ?? [];
  const totalItems = ns.tables.length + functions.length;
  return (
    <>
      <div
        className="tree-node"
        onClick={() => setOpen((o) => !o)}
        onContextMenu={openMenuFor(() => buildSchemaMenu(dbName, ns.name))}
      >
        <span className="tree-indent" />
        <span className="tree-indent" />
        <span className={`tree-chevron ${open ? "open" : ""}`}>▶</span>
        <span className="tree-icon">📁</span>
        <span className="tree-label">{ns.name}</span>
        <span className="tree-count">{totalItems}</span>
      </div>
      {open && ns.tables.map((t) => (
        <TableNode key={t.name} table={t} schemaName={ns.name} dbName={dbName} />
      ))}
      {open && functions.length > 0 && (
        <>
          {/* Functions section header */}
          <div className="tree-node" style={{ paddingLeft: 0, opacity: 0.6, pointerEvents: "none" }}>
            <span className="tree-indent" />
            <span className="tree-indent" />
            <span className="tree-indent" />
            <span className="tree-icon" style={{ fontSize: 10 }}>⚙</span>
            <span className="tree-label" style={{ fontSize: 10, textTransform: "uppercase", letterSpacing: 1 }}>Functions</span>
          </div>
          {functions.map((f) => (
            <FunctionNode key={f.name} fn={f} schemaName={ns.name} />
          ))}
        </>
      )}
    </>
  );
}

function DatabaseNode({ db }: { db: SchemaDatabase }) {
  const [open, setOpen] = useState(true);
  return (
    <>
      <div
        className="tree-node"
        onClick={() => setOpen((o) => !o)}
        onContextMenu={openMenuFor(() => buildDatabaseMenu(db.name))}
      >
        <span className="tree-indent" />
        <span className={`tree-chevron ${open ? "open" : ""}`}>▶</span>
        <span className="tree-icon">🗄</span>
        <span className="tree-label">{db.name}</span>
        <span className="tree-badge">{db.schemas.length} schemas</span>
      </div>
      {open && db.schemas.map((ns) => (
        <SchemaNode key={ns.name} ns={ns} dbName={db.name} />
      ))}
    </>
  );
}

export function SchemaTree() {
  const schema = useConnectionStore((s) => s.schema);
  const databases = schema?.databases ?? [];

  if (!schema) {
    return (
      <div style={{ padding: "16px 12px", color: "var(--text-3)", fontSize: 12 }}>
        Connect to a server to browse schema.
      </div>
    );
  }

  if (databases.length === 0) {
    return (
      <div style={{ padding: "16px 12px", color: "var(--text-3)", fontSize: 12 }}>
        No databases found.
      </div>
    );
  }

  return (
    <div>
      {databases.map((db) => (
        <DatabaseNode key={db.name} db={db} />
      ))}
    </div>
  );
}
