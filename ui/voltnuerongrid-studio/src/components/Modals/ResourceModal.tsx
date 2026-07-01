import { useState, useMemo } from "react";
import { useModalStore, type ResourceModalKind } from "@/store/modal";
import { useEditorStore } from "@/store/editor";
import type { SchemaColumn, SchemaTable } from "@/api/studio-client";
import { useConnectionStore } from "@/store/connection";
import { StudioApiClient } from "@/api/studio-client";
import { useToastStore } from "@/store/toast";
import { useSchema } from "@/hooks/useSchema";

// ─── Column draft used by CreateTableForm ─────────────────────
interface ColDraft {
  name: string;
  type: string;
  pk: boolean;
  nullable: boolean;
  defaultValue: string;
}

const TYPE_CHOICES = [
  "INT", "BIGINT", "SMALLINT", "DECIMAL(10,2)",
  "VARCHAR(255)", "TEXT",
  "BOOLEAN",
  "DATE", "TIMESTAMP",
  "JSON", "BLOB", "UUID",
];

const TITLES: Record<ResourceModalKind, string> = {
  "create-database": "Create Database",
  "drop-database":   "Drop Database",
  "create-schema":   "Create Schema",
  "drop-schema":     "Drop Schema",
  "create-table":    "Create Table",
  "create-view":     "Create View",
  "create-function": "Create Function",
  "create-trigger":  "Create Trigger",
  "create-event":    "Create Event",
  "import-tables":   "Import Tables",
  "export-tables":   "Export Tables",
  "export-view":     "Export Data",
  "export-functions": "Export Functions",
  "export-triggers": "Export Triggers",
  "export-events":   "Export Events",
  "drop-table":      "Drop Table",
  "truncate-table":  "Truncate Table",
  "rename-table":    "Rename",
  "edit-column":     "Edit Column",
  "drop-column":     "Drop Column",
  "create-user":     "Create User",
  "drop-user":       "Drop User",
  "create-role":     "Create Role",
  "grant-role":      "Grant Role",
  "view-ddl":        "DDL",
  "generate-insert": "Generate INSERT",
};

// ─── Root modal shell ─────────────────────────────────────────

export function ResourceModal() {
  const ctx = useModalStore((s) => s.current);
  const close = useModalStore((s) => s.close);
  const openSqlTab = useEditorStore((s) => s.openSqlTab);
  const getActive = useConnectionStore((s) => s.getActive);
  const getActiveKey = useConnectionStore((s) => s.getActiveKey);
  const showToast = useToastStore((s) => s.show);
  const { refresh } = useSchema();

  if (!ctx) return null;

  const title = TITLES[ctx.kind] ?? "Resource";
  const isDanger = ctx.kind.startsWith("drop-") || ctx.kind === "truncate-table";

  function runSql(sql: string, fileName: string) {
    openSqlTab(sql, fileName);
    close();
  }

  async function executeSql(sql: string, successMessage: string) {
    const conn = getActive();
    if (!conn) {
      showToast("No active connection", "error");
      return;
    }
    const client = new StudioApiClient({
      baseUrl: conn.baseUrl,
      adminApiKey: conn.mode === "admin" ? getActiveKey() : undefined,
      operatorId: conn.operatorId,
      tenantId: conn.tenantId,
      userId: conn.userId,
      database: conn.database,
    });
    try {
      const result = await client.executeSql({ sql_batch: sql });
      if (result.status !== "ok") {
        showToast("Operation failed", "error");
        return;
      }
      await refresh();
      showToast(successMessage, "success");
      close();
    } catch (err) {
      showToast(String(err), "error");
    }
  }

  async function createDatabase(name: string, owner?: string, description?: string) {
    const conn = getActive();
    if (!conn) {
      showToast("No active connection", "error");
      return;
    }
    const client = new StudioApiClient({
      baseUrl: conn.baseUrl,
      adminApiKey: conn.mode === "admin" ? getActiveKey() : undefined,
      operatorId: conn.operatorId,
      tenantId: conn.tenantId,
      userId: conn.userId,
      database: conn.database,
    });
    try {
      await client.createDatabase({ name, owner: owner || undefined, description: description || undefined, if_not_exists: true });
      await refresh();
      showToast(`Database ${name} created`, "success");
      close();
    } catch (err) {
      showToast(String(err), "error");
    }
  }

  return (
    <div className="overlay" onClick={(e) => e.target === e.currentTarget && close()}>
      <div className="conn-panel" style={{ width: ctx.kind === "create-table" ? 720 : 520 }}>
        <div className="conn-panel-header">
          <div
            className="logo-icon"
            style={{
              width: 28, height: 28, fontSize: 14,
              background: isDanger
                ? "linear-gradient(135deg,#ef4444,#9333ea)"
                : "linear-gradient(135deg,var(--brand-cyan),var(--brand-purple))",
            }}
          >
            {isDanger ? "!" : "+"}
          </div>
          <span className="conn-panel-title">{title}</span>
          {ctx.target && (
            <span className="mono" style={{ marginLeft: 8, fontSize: 11, color: "var(--text-3)" }}>
              {ctx.target}
            </span>
          )}
          <button className="conn-panel-close" onClick={close}>✕</button>
        </div>

        <div className="conn-panel-body">
          {renderBody(ctx, runSql, executeSql, createDatabase)}
        </div>
      </div>
    </div>
  );
}

// ─── Body dispatcher ──────────────────────────────────────────

type CtxType = NonNullable<ReturnType<typeof useModalStore.getState>["current"]>;
type RunSqlFn = (sql: string, fileName: string) => void;
type ExecuteSqlFn = (sql: string, successMessage: string) => Promise<void>;
type CreateDatabaseFn = (name: string, owner?: string, description?: string) => Promise<void>;
type ExportFormat = "csv" | "parquet" | "excel";

function renderBody(ctx: CtxType, runSql: RunSqlFn, executeSql: ExecuteSqlFn, createDatabase: CreateDatabaseFn) {
  switch (ctx.kind) {
    case "create-database": return <CreateDatabaseForm onSubmit={createDatabase} />;
    case "drop-database":   return <DropForm what="DATABASE" target={ctx.target!} onSubmit={runSql} />;
    case "create-schema":   return <CreateSchemaForm db={ctx.target!} onSubmit={executeSql} />;
    case "drop-schema":     return <DropForm what="SCHEMA" target={ctx.target!} onSubmit={runSql} />;
    case "create-table":    return <CreateTableForm target={ctx.target!} onSubmit={executeSql} />;
    case "create-view":     return <CreateViewForm target={ctx.target!} onSubmit={executeSql} />;
    case "create-function": return <CreateFunctionForm target={ctx.target!} onSubmit={executeSql} />;
    case "create-trigger":  return <CreateTriggerForm target={ctx.target!} onSubmit={executeSql} />;
    case "create-event":    return <CreateEventForm target={ctx.target!} onSubmit={executeSql} />;
    case "import-tables":   return <ImportTablesForm target={ctx.target!} onSubmit={executeSql} />;
    case "export-tables":   return <ExportTablesForm target={ctx.target!} onSubmit={runSql} />;
    case "export-view":     return <ExportViewForm target={ctx.target!} onSubmit={runSql} />;
    case "export-functions": return <ExportDefinitionsForm title="Functions" target={ctx.target!} kind="FUNCTION" onSubmit={runSql} />;
    case "export-triggers":  return <ExportDefinitionsForm title="Triggers" target={ctx.target!} kind="TRIGGER" onSubmit={runSql} />;
    case "export-events":    return <ExportDefinitionsForm title="Events" target={ctx.target!} kind="EVENT" onSubmit={runSql} />;
    case "drop-table":      return <DropForm what="TABLE" target={ctx.target!} onSubmit={runSql} />;
    case "truncate-table":  return <TruncateForm target={ctx.target!} onSubmit={runSql} />;
    case "rename-table":    return <RenameForm kind={(ctx.payload?.kind as string) || "table"} target={ctx.target!} onSubmit={runSql} />;
    case "edit-column":     return <EditColumnForm target={ctx.target!} col={ctx.payload?.col as SchemaColumn} onSubmit={runSql} />;
    case "drop-column":     return <DropColumnForm target={ctx.target!} col={ctx.payload?.col as SchemaColumn} onSubmit={runSql} />;
    case "create-user":     return <CreateUserForm onSubmit={runSql} />;
    case "drop-user":       return <DropForm what="USER" target={ctx.target!} onSubmit={runSql} />;
    case "create-role":     return <CreateRoleForm onSubmit={runSql} />;
    case "grant-role":      return <GrantRoleForm user={ctx.target!} onSubmit={runSql} />;
    case "view-ddl":        return <ViewDdlForm target={ctx.target!} payload={ctx.payload} onSubmit={runSql} />;
    case "generate-insert": return <GenerateInsertForm target={ctx.target!} payload={ctx.payload} onSubmit={runSql} />;
  }
}

// ─── Shared sub-components ────────────────────────────────────

function Field({ label, children, full = false }: {
  label: string;
  children: React.ReactNode;
  full?: boolean;
}) {
  return (
    <div className={`form-field${full ? " full" : ""}`}>
      <label className="form-label">{label}</label>
      {children}
    </div>
  );
}

function Footer({ onCancel, onSubmit, label = "Generate SQL", danger = false, disabled = false }: {
  onCancel: () => void;
  onSubmit: () => void;
  label?: string;
  danger?: boolean;
  disabled?: boolean;
}) {
  return (
    <div className="conn-panel-footer" style={{ borderTop: "1px solid var(--border)", marginTop: 14 }}>
      <div style={{ flex: 1 }} />
      <button
        className="btn-wide secondary"
        style={{ width: 110 }}
        onClick={onCancel}
      >
        Cancel
      </button>
      <button
        className="btn-wide primary"
        style={{ width: 160, background: danger ? "var(--red)" : undefined }}
        onClick={onSubmit}
        disabled={disabled}
      >
        {label}
      </button>
    </div>
  );
}

function extractRowsForExport(res: Awaited<ReturnType<StudioApiClient["executeSql"]>>): {
  columns: string[];
  rows: Array<Record<string, unknown>>;
} {
  if ((res.columns ?? []).length > 0 && (res.rows ?? []).length > 0) {
    return {
      columns: (res.columns ?? []).map((col) => col.name),
      rows: res.rows ?? [],
    };
  }

  const rows = (res.oltp_rows ?? []).map((row) => row.data);
  const columns = [...new Set(rows.flatMap((row) => Object.keys(row).filter((key) => !key.startsWith("__"))))]
    .sort((a, b) => a.localeCompare(b));
  return {
    columns,
    rows: rows.map((row) => {
      const out: Record<string, unknown> = {};
      for (const col of columns) out[col] = row[col] ?? null;
      return out;
    }),
  };
}

function csvEscape(value: unknown): string {
  const text = value == null ? "" : String(value);
  return text.includes(",") || text.includes("\"") || text.includes("\n")
    ? `"${text.replace(/\"/g, '""')}"`
    : text;
}

function download(content: string, fileName: string, mimeType: string): void {
  const blob = new Blob([content], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 5000);
}

async function exportQuery(
  client: StudioApiClient,
  sql: string,
  baseName: string,
  format: ExportFormat,
  showToast: (message: string, kind?: "info" | "success" | "error") => void,
): Promise<void> {
  const res = await client.executeSql({ sql_batch: sql });
  if (res.status !== "ok") {
    showToast("Export failed", "error");
    return;
  }
  const { columns, rows } = extractRowsForExport(res);
  if (columns.length === 0) {
    showToast("No rows available to export", "error");
    return;
  }
  const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
  const csv = [
    columns.join(","),
    ...rows.map((row) => columns.map((col) => csvEscape(row[col])).join(",")),
  ].join("\n");

  if (format === "csv") {
    download(csv, `${baseName}-${stamp}.csv`, "text/csv;charset=utf-8;");
    showToast("CSV export complete", "success");
    return;
  }

  if (format === "excel") {
    download(csv, `${baseName}-${stamp}.xls`, "application/vnd.ms-excel;charset=utf-8;");
    showToast("Excel export complete", "success");
    return;
  }

  download(JSON.stringify({ columns, rows }, null, 2), `${baseName}-${stamp}.parquet.json`, "application/json");
  showToast("Parquet binary export is not available in browser mode. Exported JSON fallback.", "info");
}

function parseCsvLine(line: string): string[] {
  const out: string[] = [];
  let current = "";
  let inQuotes = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (ch === '"') {
      if (inQuotes && line[i + 1] === '"') {
        current += '"';
        i++;
      } else {
        inQuotes = !inQuotes;
      }
    } else if (ch === "," && !inQuotes) {
      out.push(current.trim());
      current = "";
    } else {
      current += ch;
    }
  }
  out.push(current.trim());
  return out;
}

// ─── Individual forms ─────────────────────────────────────────

function CreateDatabaseForm({ onSubmit }: { onSubmit: CreateDatabaseFn }) {
  const close = useModalStore((s) => s.close);
  const [name, setName] = useState("");
  const [owner, setOwner] = useState("");
  const [description, setDescription] = useState("");

  async function handleSubmit() {
    if (!name.trim()) return;
    await onSubmit(name.trim().toLowerCase(), owner.trim(), description.trim());
  }

  return (
    <>
      <div style={{ color: "var(--text-2)", fontSize: 12.5, lineHeight: 1.55, padding: "4px 2px 8px" }}>
        Creates a clean database directly with no template tables.
      </div>
      <div className="form-row">
        <Field label="Database Name">
          <input
            className="form-input"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="e.g. analytics"
            autoFocus
          />
        </Field>
        <Field label="Owner (optional)">
          <input
            className="form-input"
            value={owner}
            onChange={(e) => setOwner(e.target.value)}
            placeholder="admin"
          />
        </Field>
      </div>
      <Field label="Description (optional)" full>
        <input
          className="form-input"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="Short purpose"
        />
      </Field>
      <div className="conn-panel-footer" style={{ borderTop: "1px solid var(--border)", marginTop: 14 }}>
        <div style={{ flex: 1 }} />
        <button className="btn-wide secondary" style={{ width: 110 }} onClick={close}>Cancel</button>
        <button className="btn-wide primary" style={{ width: 160 }} onClick={handleSubmit} disabled={!name.trim()}>
          Create Database
        </button>
      </div>
    </>
  );
}

function CreateSchemaForm({ db, onSubmit }: { db: string; onSubmit: ExecuteSqlFn }) {
  const close = useModalStore((s) => s.close);
  const [name, setName] = useState("");
  const [owner, setOwner] = useState("");

  return (
    <>
      <div className="form-row">
        <Field label="Schema Name">
          <input
            className="form-input"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="e.g. reporting"
            autoFocus
          />
        </Field>
        <Field label="Owner (optional)">
          <input
            className="form-input"
            value={owner}
            onChange={(e) => setOwner(e.target.value)}
            placeholder="user or role"
          />
        </Field>
      </div>
      <Footer
        onCancel={close}
        label="Create Schema"
        onSubmit={async () => {
          if (!name) return;
          const ownerClause = owner ? ` AUTHORIZATION ${owner}` : "";
          await onSubmit(`CREATE SCHEMA ${db}.${name}${ownerClause};`, `Schema ${name} created`);
        }}
      />
    </>
  );
}

function CreateTableForm({ target, onSubmit }: { target: string; onSubmit: ExecuteSqlFn }) {
  const close = useModalStore((s) => s.close);
  const [name, setName] = useState("");
  const [cols, setCols] = useState<ColDraft[]>([
    { name: "id", type: "BIGINT", pk: true, nullable: false, defaultValue: "" },
    { name: "created_at", type: "TIMESTAMP", pk: false, nullable: false, defaultValue: "CURRENT_TIMESTAMP" },
  ]);

  function update(i: number, patch: Partial<ColDraft>) {
    setCols((cs) => cs.map((c, idx) => (idx === i ? { ...c, ...patch } : c)));
  }

  function addCol() {
    setCols((cs) => [
      ...cs,
      { name: "", type: "VARCHAR(255)", pk: false, nullable: true, defaultValue: "" },
    ]);
  }

  function removeCol(i: number) {
    setCols((cs) => cs.filter((_, idx) => idx !== i));
  }

  return (
    <>
      <div className="form-row">
        <Field label="Table Name" full>
          <input
            className="form-input"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="e.g. orders"
            autoFocus
          />
        </Field>
      </div>

      <div className="detail-title" style={{ marginTop: 8 }}>Columns</div>
      <div
        className="col-list"
        style={{
          background: "var(--bg-2)",
          border: "1px solid var(--border)",
          borderRadius: "var(--r-sm)",
          padding: 6,
          maxHeight: 280,
          overflowY: "auto",
        }}
      >
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "1.5fr 1.5fr 50px 50px 1.2fr 28px",
            gap: 6,
            padding: "2px 4px",
            fontSize: 10,
            color: "var(--text-3)",
            textTransform: "uppercase",
            letterSpacing: ".06em",
            fontWeight: 700,
          }}
        >
          <span>Name</span>
          <span>Type</span>
          <span>PK</span>
          <span>Null</span>
          <span>Default</span>
          <span />
        </div>
        {cols.map((col, i) => (
          <div
            key={i}
            style={{
              display: "grid",
              gridTemplateColumns: "1.5fr 1.5fr 50px 50px 1.2fr 28px",
              gap: 6,
              padding: "3px 4px",
              alignItems: "center",
            }}
          >
            <input
              className="form-input"
              style={{ height: 26, fontSize: 11.5 }}
              value={col.name}
              onChange={(e) => update(i, { name: e.target.value })}
              placeholder="column"
            />
            <select
              className="form-select"
              style={{ height: 26, fontSize: 11 }}
              value={col.type}
              onChange={(e) => update(i, { type: e.target.value })}
            >
              {TYPE_CHOICES.map((t) => (
                <option key={t}>{t}</option>
              ))}
            </select>
            <input
              type="checkbox"
              checked={col.pk}
              onChange={(e) => update(i, { pk: e.target.checked })}
            />
            <input
              type="checkbox"
              checked={col.nullable}
              onChange={(e) => update(i, { nullable: e.target.checked })}
            />
            <input
              className="form-input"
              style={{ height: 26, fontSize: 11.5 }}
              value={col.defaultValue}
              onChange={(e) => update(i, { defaultValue: e.target.value })}
              placeholder="—"
            />
            <button
              className="btn btn-sm"
              style={{ padding: "0 6px" }}
              onClick={() => removeCol(i)}
              title="Remove"
            >
              ✕
            </button>
          </div>
        ))}
      </div>
      <button
        className="btn btn-sm"
        style={{ alignSelf: "flex-start", marginTop: 6 }}
        onClick={addCol}
      >
        + Add Column
      </button>

      <Footer
        onCancel={close}
        label="Create Table"
        onSubmit={async () => {
          if (!name || cols.length === 0) return;
          const colDdl = cols
            .filter((col) => col.name.trim())
            .map((col) => {
              const parts = [`  ${col.name} ${col.type}`];
              if (!col.nullable) parts.push("NOT NULL");
              if (col.defaultValue) parts.push(`DEFAULT ${col.defaultValue}`);
              return parts.join(" ");
            });
          const pks = cols.filter((col) => col.pk).map((col) => col.name);
          if (pks.length) colDdl.push(`  PRIMARY KEY (${pks.join(", ")})`);
          await onSubmit(`CREATE TABLE ${target}.${name} (\n${colDdl.join(",\n")}\n);`, `Table ${name} created`);
        }}
      />
    </>
  );
}

function CreateViewForm({ target, onSubmit }: { target: string; onSubmit: ExecuteSqlFn }) {
  const close = useModalStore((s) => s.close);
  const [mode, setMode] = useState<"sql" | "builder">("sql");
  const [name, setName] = useState("");
  const [source, setSource] = useState("");
  const [columns, setColumns] = useState("*");
  const [where, setWhere] = useState("");
  const [sql, setSql] = useState("CREATE VIEW my_view AS\nSELECT *\nFROM my_table;");

  const sqlText =
    mode === "sql"
      ? sql
      : `CREATE VIEW ${target}.${name} AS\nSELECT ${columns || "*"}\nFROM ${target}.${source}${where ? `\nWHERE ${where}` : ""};`;

  return (
    <>
      <div style={{ display: "flex", gap: 8 }}>
        <button className="btn btn-sm" onClick={() => setMode("sql")} style={{ background: mode === "sql" ? "var(--bg-active)" : undefined }}>SQL</button>
        <button className="btn btn-sm" onClick={() => setMode("builder")} style={{ background: mode === "builder" ? "var(--bg-active)" : undefined }}>Builder</button>
      </div>

      {mode === "builder" && (
        <>
          <div className="form-row">
            <Field label="View Name">
              <input className="form-input" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. active_orders" />
            </Field>
            <Field label="Source Table">
              <input className="form-input" value={source} onChange={(e) => setSource(e.target.value)} placeholder="e.g. orders" />
            </Field>
          </div>
          <div className="form-row">
            <Field label="Columns">
              <input className="form-input" value={columns} onChange={(e) => setColumns(e.target.value)} placeholder="* or id, name" />
            </Field>
            <Field label="Where (optional)">
              <input className="form-input" value={where} onChange={(e) => setWhere(e.target.value)} placeholder="status = 'ACTIVE'" />
            </Field>
          </div>
        </>
      )}

      {mode === "sql" && (
        <textarea
          className="form-input mono"
          value={sql}
          onChange={(e) => setSql(e.target.value)}
          rows={10}
          style={{ height: "auto", padding: 10, fontSize: 12, lineHeight: 1.5, resize: "vertical" }}
        />
      )}

      <Footer
        onCancel={close}
        label="Create View"
        onSubmit={async () => {
          await onSubmit(sqlText, `View ${name || "created"} created`);
        }}
      />
    </>
  );
}

function CreateFunctionForm({ target, onSubmit }: { target: string; onSubmit: ExecuteSqlFn }) {
  const close = useModalStore((s) => s.close);
  const [name, setName] = useState("");
  const [language, setLanguage] = useState<"sql" | "python" | "javascript">("sql");
  const [args, setArgs] = useState("");
  const [returns, setReturns] = useState("TEXT");
  const [body, setBody] = useState("RETURN 'ok';");

  return (
    <>
      <div className="form-row">
        <Field label="Function Name">
          <input className="form-input" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. compute_score" />
        </Field>
        <Field label="Language">
          <select className="form-select" value={language} onChange={(e) => setLanguage(e.target.value as "sql" | "python" | "javascript")}>
            <option value="sql">SQL</option>
            <option value="python">Python</option>
            <option value="javascript">JavaScript</option>
          </select>
        </Field>
      </div>
      <div className="form-row">
        <Field label="Arguments">
          <input className="form-input" value={args} onChange={(e) => setArgs(e.target.value)} placeholder="a INT, b INT" />
        </Field>
        <Field label="Returns">
          <input className="form-input" value={returns} onChange={(e) => setReturns(e.target.value)} />
        </Field>
      </div>
      <Field label="Body" full>
        <textarea className="form-input mono" value={body} onChange={(e) => setBody(e.target.value)} rows={8} style={{ height: "auto", padding: 10, fontSize: 12, lineHeight: 1.5, resize: "vertical" }} />
      </Field>
      <Footer
        onCancel={close}
        label="Create Function"
        onSubmit={async () => {
          if (!name.trim()) return;
          const normalizedBody = body.replace(/;/g, " ").trim() || "RETURN NULL";
          const sql = `CREATE FUNCTION ${target}.${name}(${args}) RETURNS ${returns}\nLANGUAGE ${language}\nAS $$\n${normalizedBody}\n$$;`;
          await onSubmit(sql, `Function ${name} created`);
        }}
      />
    </>
  );
}

function CreateTriggerForm({ target, onSubmit }: { target: string; onSubmit: ExecuteSqlFn }) {
  const close = useModalStore((s) => s.close);
  const [name, setName] = useState("");
  const [tableName, setTableName] = useState("");
  const [timing, setTiming] = useState("BEFORE");
  const [eventType, setEventType] = useState("INSERT");
  const [body, setBody] = useState("SELECT 1;");

  return (
    <>
      <div className="form-row">
        <Field label="Trigger Name">
          <input className="form-input" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. orders_audit_trg" />
        </Field>
        <Field label="Table">
          <input className="form-input" value={tableName} onChange={(e) => setTableName(e.target.value)} placeholder="e.g. orders" />
        </Field>
      </div>
      <div className="form-row">
        <Field label="Timing">
          <select className="form-select" value={timing} onChange={(e) => setTiming(e.target.value)}>
            <option>BEFORE</option>
            <option>AFTER</option>
          </select>
        </Field>
        <Field label="Event">
          <select className="form-select" value={eventType} onChange={(e) => setEventType(e.target.value)}>
            <option>INSERT</option>
            <option>UPDATE</option>
            <option>DELETE</option>
          </select>
        </Field>
      </div>
      <Field label="Body" full>
        <textarea className="form-input mono" value={body} onChange={(e) => setBody(e.target.value)} rows={6} style={{ height: "auto", padding: 10, fontSize: 12, lineHeight: 1.5, resize: "vertical" }} />
      </Field>
      <Footer
        onCancel={close}
        label="Create Trigger"
        onSubmit={async () => {
          if (!name.trim() || !tableName.trim()) return;
          const normalizedBody = body.replace(/;/g, " ").trim() || "SELECT 1";
          const sql = `CREATE TRIGGER ${name}\n${timing} ${eventType} ON ${target}.${tableName}\nFOR EACH ROW\nBEGIN\n${normalizedBody}\nEND;`;
          await onSubmit(sql, `Trigger ${name} created`);
        }}
      />
    </>
  );
}

function CreateEventForm({ target, onSubmit }: { target: string; onSubmit: ExecuteSqlFn }) {
  const close = useModalStore((s) => s.close);
  const [name, setName] = useState("");
  const [schedule, setSchedule] = useState("EVERY 1 DAY");
  const [body, setBody] = useState("SELECT 1;");

  return (
    <>
      <div className="form-row">
        <Field label="Event Name">
          <input className="form-input" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. nightly_rollup" />
        </Field>
        <Field label="Schedule">
          <input className="form-input" value={schedule} onChange={(e) => setSchedule(e.target.value)} />
        </Field>
      </div>
      <Field label="Body" full>
        <textarea className="form-input mono" value={body} onChange={(e) => setBody(e.target.value)} rows={6} style={{ height: "auto", padding: 10, fontSize: 12, lineHeight: 1.5, resize: "vertical" }} />
      </Field>
      <Footer
        onCancel={close}
        label="Create Event"
        onSubmit={async () => {
          if (!name.trim()) return;
          const normalizedBody = body.replace(/;/g, " ").trim() || "SELECT 1";
          const sql = `CREATE EVENT ${name}\nON SCHEDULE ${schedule}\nDO\n${normalizedBody};`;
          await onSubmit(sql, `Event ${name} created`);
        }}
      />
    </>
  );
}

function ImportTablesForm({ target, onSubmit }: { target: string; onSubmit: ExecuteSqlFn }) {
  const close = useModalStore((s) => s.close);
  const showToast = useToastStore((s) => s.show);
  const [tableName, setTableName] = useState("");
  const [format, setFormat] = useState<ExportFormat>("csv");
  const [file, setFile] = useState<File | null>(null);

  return (
    <>
      <div className="form-row">
        <Field label="Table Name">
          <input className="form-input" value={tableName} onChange={(e) => setTableName(e.target.value)} placeholder="e.g. customers" />
        </Field>
        <Field label="Source Format">
          <select className="form-select" value={format} onChange={(e) => setFormat(e.target.value as ExportFormat)}>
            <option value="csv">CSV</option>
            <option value="parquet">Parquet</option>
            <option value="excel">Excel</option>
          </select>
        </Field>
      </div>
      <Field label="File" full>
        <input className="form-input" type="file" accept=".csv,.parquet,.xls,.xlsx" onChange={(e) => setFile(e.target.files?.[0] ?? null)} />
      </Field>
      <Footer
        onCancel={close}
        label="Import"
        disabled={!tableName.trim() || !file}
        onSubmit={async () => {
          if (!file || !tableName.trim()) return;
          if (format !== "csv") {
            showToast("Parquet and Excel import are not available in browser mode yet. Use CSV for now.", "info");
            return;
          }
          const text = await file.text();
          const lines = text.split(/\r?\n/).filter((line) => line.trim().length > 0);
          if (lines.length < 2) {
            showToast("CSV requires a header and at least one row", "error");
            return;
          }
          const cols = parseCsvLine(lines[0]).map((col) => col.replace(/[^a-zA-Z0-9_]/g, "_").toLowerCase());
          const createSql = `CREATE TABLE IF NOT EXISTS ${target}.${tableName} (${cols.map((col) => `${col} TEXT`).join(", ")});`;
          const values = lines.slice(1).map((line) => `(${parseCsvLine(line).map((value) => `'${value.replace(/'/g, "''")}'`).join(", ")})`);
          const insertSql = values.length > 0
            ? `INSERT INTO ${target}.${tableName} (${cols.join(", ")}) VALUES\n${values.join(",\n")};`
            : "";
          await onSubmit(`${createSql}\n${insertSql}`.trim(), `Imported ${values.length} rows into ${tableName}`);
        }}
      />
    </>
  );
}

function ExportTablesForm({ target }: { target: string; onSubmit: RunSqlFn }) {
  const close = useModalStore((s) => s.close);
  const schema = useConnectionStore((s) => s.schema);
  const getActive = useConnectionStore((s) => s.getActive);
  const getActiveKey = useConnectionStore((s) => s.getActiveKey);
  const showToast = useToastStore((s) => s.show);
  const [format, setFormat] = useState<ExportFormat>("csv");
  const [scope, setScope] = useState<"all" | "selected">("all");
  const [selected, setSelected] = useState<string[]>([]);

  const [dbName, schemaName] = target.split(".");
  const tables = useMemo(() => {
    const db = schema?.databases.find((d) => d.name === dbName);
    const ns = db?.schemas.find((s) => s.name === schemaName);
    return (ns?.tables ?? []).map((table) => table.name.split(".").pop() ?? table.name);
  }, [schema, dbName, schemaName]);

  async function handleExport() {
    const conn = getActive();
    if (!conn) return;
    const client = new StudioApiClient({
      baseUrl: conn.baseUrl,
      adminApiKey: conn.mode === "admin" ? getActiveKey() : undefined,
      operatorId: conn.operatorId,
      tenantId: conn.tenantId,
      userId: conn.userId,
      database: conn.database,
    });
    const names = scope === "all" ? tables : selected;
    if (names.length === 0) {
      showToast("No tables selected", "error");
      return;
    }
    for (const tableName of names) {
      await exportQuery(client, `SELECT * FROM ${schemaName}.${tableName};`, `${schemaName}-${tableName}`, format, showToast);
    }
    close();
  }

  return (
    <>
      <div className="form-row">
        <Field label="Export Format">
          <select className="form-select" value={format} onChange={(e) => setFormat(e.target.value as ExportFormat)}>
            <option value="csv">CSV</option>
            <option value="parquet">Parquet</option>
            <option value="excel">Excel</option>
          </select>
        </Field>
        <Field label="Scope">
          <select className="form-select" value={scope} onChange={(e) => setScope(e.target.value as "all" | "selected") }>
            <option value="all">All Tables</option>
            <option value="selected">Selected Tables</option>
          </select>
        </Field>
      </div>
      {scope === "selected" && (
        <div style={{ maxHeight: 200, overflowY: "auto", border: "1px solid var(--border)", borderRadius: "var(--r-sm)", padding: 8 }}>
          {tables.map((table) => (
            <label key={table} style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, marginBottom: 6 }}>
              <input
                type="checkbox"
                checked={selected.includes(table)}
                onChange={(e) =>
                  setSelected((prev) =>
                    e.target.checked ? [...prev, table] : prev.filter((name) => name !== table)
                  )
                }
              />
              {table}
            </label>
          ))}
        </div>
      )}
      <Footer onCancel={close} label="Export" onSubmit={handleExport} />
    </>
  );
}

function ExportViewForm({ target }: { target: string; onSubmit: RunSqlFn }) {
  const close = useModalStore((s) => s.close);
  const getActive = useConnectionStore((s) => s.getActive);
  const getActiveKey = useConnectionStore((s) => s.getActiveKey);
  const showToast = useToastStore((s) => s.show);
  const [format, setFormat] = useState<ExportFormat>("csv");

  return (
    <>
      <Field label="Export Format" full>
        <select className="form-select" value={format} onChange={(e) => setFormat(e.target.value as ExportFormat)}>
          <option value="csv">CSV</option>
          <option value="parquet">Parquet</option>
          <option value="excel">Excel</option>
        </select>
      </Field>
      <Footer
        onCancel={close}
        label="Export"
        onSubmit={async () => {
          const conn = getActive();
          if (!conn) return;
          const client = new StudioApiClient({
            baseUrl: conn.baseUrl,
            adminApiKey: conn.mode === "admin" ? getActiveKey() : undefined,
            operatorId: conn.operatorId,
            tenantId: conn.tenantId,
            userId: conn.userId,
            database: conn.database,
          });
          const parts = target.split(".");
          const schemaName = parts[1];
          const viewName = parts[2];
          await exportQuery(client, `SELECT * FROM ${schemaName}.${viewName};`, `${schemaName}-${viewName}`, format, showToast);
          close();
        }}
      />
    </>
  );
}

function ExportDefinitionsForm({
  title,
  target,
  kind,
}: {
  title: string;
  target: string;
  kind: "FUNCTION" | "TRIGGER" | "EVENT";
  onSubmit: RunSqlFn;
}) {
  const close = useModalStore((s) => s.close);
  const schema = useConnectionStore((s) => s.schema);
  const showToast = useToastStore((s) => s.show);
  const [dbName, schemaName] = target.split(".");

  return (
    <>
      <div style={{ color: "var(--text-2)", fontSize: 12.5, lineHeight: 1.55 }}>
        Export all {title.toLowerCase()} definitions from {schemaName}.
      </div>
      <Footer
        onCancel={close}
        label={`Export ${title}`}
        onSubmit={() => {
          const db = schema?.databases.find((d) => d.name === dbName);
          const ns = db?.schemas.find((s) => s.name === schemaName);
          const payload =
            kind === "FUNCTION"
              ? ns?.functions ?? []
              : kind === "TRIGGER"
              ? ns?.triggers ?? []
              : ns?.events ?? [];
          const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
          download(
            JSON.stringify(payload, null, 2),
            `${schemaName}-${title.toLowerCase()}-${stamp}.json`,
            "application/json",
          );
          showToast(`${title} export complete`, "success");
          close();
        }}
      />
    </>
  );
}

function DropForm({
  what,
  target,
  onSubmit,
}: {
  what: string;
  target: string;
  onSubmit: RunSqlFn;
}) {
  const close = useModalStore((s) => s.close);
  const [confirmText, setConfirmText] = useState("");
  const [cascade, setCascade] = useState(false);
  const shortName = target.split(".").pop() ?? target;
  const matches = confirmText === shortName;

  return (
    <>
      <div
        style={{
          background: "#ef444411",
          border: "1px solid #ef444433",
          borderRadius: "var(--r-sm)",
          padding: "10px 12px",
          color: "var(--red)",
          fontSize: 12.5,
          marginBottom: 10,
        }}
      >
        ⚠ This will permanently drop <strong>{target}</strong>. This action cannot be undone.
      </div>
      <Field label={`Type "${shortName}" to confirm`} full>
        <input
          className="form-input"
          value={confirmText}
          onChange={(e) => setConfirmText(e.target.value)}
          autoFocus
        />
      </Field>
      {what !== "USER" && (
        <label
          style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, marginTop: 8, color: "var(--text-2)" }}
        >
          <input
            type="checkbox"
            checked={cascade}
            onChange={(e) => setCascade(e.target.checked)}
          />
          CASCADE — drop dependent objects
        </label>
      )}
      <Footer
        onCancel={close}
        danger
        label={`Drop ${what}`}
        onSubmit={() => {
          if (!matches) return;
          const cascadeClause = cascade ? " CASCADE" : "";
          onSubmit(
            `DROP ${what} IF EXISTS ${target}${cascadeClause};`,
            `drop_${target}.sql`
          );
        }}
      />
    </>
  );
}

function TruncateForm({ target, onSubmit }: { target: string; onSubmit: RunSqlFn }) {
  const close = useModalStore((s) => s.close);
  const [confirmText, setConfirmText] = useState("");
  const [restart, setRestart] = useState(true);
  const shortName = target.split(".").pop() ?? target;
  const matches = confirmText === shortName;

  return (
    <>
      <div
        style={{
          background: "#eab30811",
          border: "1px solid #eab30833",
          borderRadius: "var(--r-sm)",
          padding: "10px 12px",
          color: "var(--yellow)",
          fontSize: 12.5,
          marginBottom: 10,
        }}
      >
        ⚠ Truncate removes ALL rows from <strong>{target}</strong>. This is fast but not transactional in some routes.
      </div>
      <Field label={`Type "${shortName}" to confirm`} full>
        <input
          className="form-input"
          value={confirmText}
          onChange={(e) => setConfirmText(e.target.value)}
          autoFocus
        />
      </Field>
      <label
        style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, marginTop: 8, color: "var(--text-2)" }}
      >
        <input
          type="checkbox"
          checked={restart}
          onChange={(e) => setRestart(e.target.checked)}
        />
        Restart identity sequences
      </label>
      <Footer
        onCancel={close}
        danger
        label="Truncate"
        onSubmit={() => {
          if (!matches) return;
          const restartClause = restart ? " RESTART IDENTITY" : "";
          onSubmit(
            `TRUNCATE TABLE ${target}${restartClause};`,
            `truncate_${target}.sql`
          );
        }}
      />
    </>
  );
}

function RenameForm({
  kind,
  target,
  onSubmit,
}: {
  kind: string;
  target: string;
  onSubmit: RunSqlFn;
}) {
  const close = useModalStore((s) => s.close);
  const [next, setNext] = useState("");
  const what = kind.toUpperCase();
  const shortName = target.split(".").pop() ?? target;

  return (
    <>
      <Field label="New Name" full>
        <input
          className="form-input"
          value={next}
          onChange={(e) => setNext(e.target.value)}
          placeholder={shortName}
          autoFocus
        />
      </Field>
      <Footer
        onCancel={close}
        onSubmit={() => {
          if (!next) return;
          const sql =
            kind === "table"
              ? `ALTER TABLE ${target} RENAME TO ${next};`
              : `ALTER ${what} ${target} RENAME TO ${next};`;
          onSubmit(sql, `rename_${target}.sql`);
        }}
      />
    </>
  );
}

function EditColumnForm({
  target,
  col,
  onSubmit,
}: {
  target: string;
  col: SchemaColumn;
  onSubmit: RunSqlFn;
}) {
  const close = useModalStore((s) => s.close);
  const [type, setType] = useState(col.data_type);
  const [nullable, setNullable] = useState(col.nullable);
  const [newName, setNewName] = useState(col.name);
  const tableTarget = target.split(".").slice(0, 3).join(".");

  return (
    <>
      <div className="form-row">
        <Field label="Column Name">
          <input
            className="form-input"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
          />
        </Field>
        <Field label="Type">
          <select
            className="form-select"
            value={type}
            onChange={(e) => setType(e.target.value)}
          >
            {[type, ...TYPE_CHOICES]
              .filter((v, i, a) => a.indexOf(v) === i)
              .map((t) => (
                <option key={t}>{t}</option>
              ))}
          </select>
        </Field>
      </div>
      <label
        style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, marginTop: 8, color: "var(--text-2)" }}
      >
        <input
          type="checkbox"
          checked={nullable}
          onChange={(e) => setNullable(e.target.checked)}
        />
        Nullable
      </label>
      <Footer
        onCancel={close}
        onSubmit={() => {
          const stmts: string[] = [];
          if (newName !== col.name)
            stmts.push(
              `ALTER TABLE ${tableTarget} RENAME COLUMN ${col.name} TO ${newName};`
            );
          if (type !== col.data_type)
            stmts.push(
              `ALTER TABLE ${tableTarget} ALTER COLUMN ${newName} TYPE ${type};`
            );
          if (nullable !== col.nullable)
            stmts.push(
              `ALTER TABLE ${tableTarget} ALTER COLUMN ${newName} ${nullable ? "DROP" : "SET"} NOT NULL;`
            );
          if (!stmts.length) return;
          onSubmit(stmts.join("\n"), `alter_${col.name}.sql`);
        }}
      />
    </>
  );
}

function DropColumnForm({
  target,
  col,
  onSubmit,
}: {
  target: string;
  col: SchemaColumn;
  onSubmit: RunSqlFn;
}) {
  const close = useModalStore((s) => s.close);
  const tableTarget = target.split(".").slice(0, 3).join(".");

  return (
    <>
      <div
        style={{
          background: "#ef444411",
          border: "1px solid #ef444433",
          borderRadius: "var(--r-sm)",
          padding: "10px 12px",
          color: "var(--red)",
          fontSize: 12.5,
          marginBottom: 10,
        }}
      >
        ⚠ Drop column <strong>{col.name}</strong> from <strong>{tableTarget}</strong>. Data in this column will be lost.
      </div>
      <Footer
        onCancel={close}
        danger
        label="Drop Column"
        onSubmit={() =>
          onSubmit(
            `ALTER TABLE ${tableTarget} DROP COLUMN ${col.name};`,
            `drop_col_${col.name}.sql`
          )
        }
      />
    </>
  );
}

function CreateUserForm({ onSubmit }: { onSubmit: RunSqlFn }) {
  const close = useModalStore((s) => s.close);
  const [name, setName] = useState("");
  const [pw, setPw] = useState("");
  const [role, setRole] = useState("readonly");

  return (
    <>
      <div className="form-row">
        <Field label="Username">
          <input
            className="form-input"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="e.g. analyst"
            autoFocus
          />
        </Field>
        <Field label="Default Role">
          <select
            className="form-select"
            value={role}
            onChange={(e) => setRole(e.target.value)}
          >
            <option value="readonly">readonly</option>
            <option value="readwrite">readwrite</option>
            <option value="dba">dba</option>
            <option value="operator">operator</option>
          </select>
        </Field>
      </div>
      <Field label="Password" full>
        <input
          className="form-input"
          type="password"
          value={pw}
          onChange={(e) => setPw(e.target.value)}
        />
      </Field>
      <Footer
        onCancel={close}
        onSubmit={() => {
          if (!name || !pw) return;
          const sql = `CREATE USER ${name} WITH PASSWORD '${pw}';\nGRANT ${role} TO ${name};`;
          onSubmit(sql, `create_user_${name}.sql`);
        }}
      />
    </>
  );
}

function CreateRoleForm({ onSubmit }: { onSubmit: RunSqlFn }) {
  const close = useModalStore((s) => s.close);
  const [name, setName] = useState("");

  return (
    <>
      <Field label="Role Name" full>
        <input
          className="form-input"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="e.g. analyst_role"
          autoFocus
        />
      </Field>
      <Footer
        onCancel={close}
        onSubmit={() => name && onSubmit(`CREATE ROLE ${name};`, `create_role_${name}.sql`)}
      />
    </>
  );
}

function GrantRoleForm({ user, onSubmit }: { user: string; onSubmit: RunSqlFn }) {
  const close = useModalStore((s) => s.close);
  const [role, setRole] = useState("readonly");

  return (
    <>
      <Field label="Role" full>
        <select
          className="form-select"
          value={role}
          onChange={(e) => setRole(e.target.value)}
        >
          <option value="readonly">readonly</option>
          <option value="readwrite">readwrite</option>
          <option value="dba">dba</option>
          <option value="operator">operator</option>
        </select>
      </Field>
      <Footer
        onCancel={close}
        onSubmit={() => onSubmit(`GRANT ${role} TO ${user};`, `grant_${user}.sql`)}
      />
    </>
  );
}

function ViewDdlForm({
  target,
  payload,
  onSubmit,
}: {
  target: string;
  payload?: Record<string, unknown>;
  onSubmit: RunSqlFn;
}) {
  const close = useModalStore((s) => s.close);

  const ddl = useMemo(() => {
    const kind = payload?.kind as string;
    if (kind === "table" && payload?.table) {
      const t = payload.table as SchemaTable;
      const lines = t.columns.map((col) => {
        const nn = col.nullable ? "" : " NOT NULL";
        const pk = col.primary_key ? " PRIMARY KEY" : "";
        return `  ${col.name} ${col.data_type}${nn}${pk}`;
      });
      return `CREATE TABLE ${target} (\n${lines.join(",\n")}\n);`;
    }
    if (typeof payload?.definition === "string" && payload.definition.trim().length > 0) {
      return payload.definition;
    }
    return `-- DDL for ${target}\n-- (server returns synthesized DDL — preview only)`;
  }, [target, payload]);

  return (
    <>
      <textarea
        className="form-input mono"
        value={ddl}
        readOnly
        rows={14}
        style={{ height: "auto", padding: 10, fontSize: 12, lineHeight: 1.5, resize: "vertical" }}
      />
      <Footer
        onCancel={close}
        label="Open in Editor"
        onSubmit={() => onSubmit(ddl, `ddl_${target}.sql`)}
      />
    </>
  );
}

// ─── Generate Insert Form ──────────────────────────────────────

function insertTypedDefault(col: SchemaColumn): string {
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

function GenerateInsertForm({
  target,
  payload,
  onSubmit,
}: {
  target: string;
  payload?: Record<string, unknown>;
  onSubmit: RunSqlFn;
}) {
  const close = useModalStore((s) => s.close);
  const [rowCount, setRowCount] = useState(1);
  const [rawInput, setRawInput] = useState("1");

  const columns = useMemo(() => (payload?.columns as SchemaColumn[]) ?? [], [payload]);
  const [schemaName, tableName] = target.split(".");

  const QUICK = [1, 5, 10, 50, 100] as const;

  function buildSql(n: number): string {
    const cols = columns.map((c) => c.name).join(", ");
    const singleRow = `(${columns.map((c) => insertTypedDefault(c)).join(", ")})`;
    const rows = Array.from({ length: n }, () => `  ${singleRow}`).join(",\n");
    return `INSERT INTO ${schemaName}.${tableName} (${cols})\nVALUES\n${rows};`;
  }

  const previewSql = useMemo(
    () => buildSql(Math.min(rowCount, 3)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [rowCount, columns, schemaName, tableName]
  );

  function handleChange(raw: string) {
    setRawInput(raw);
    const n = parseInt(raw, 10);
    if (!isNaN(n) && n >= 1 && n <= 1000) setRowCount(n);
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      {/* Row count picker */}
      <div>
        <label style={{ display: "block", fontSize: 12, fontWeight: 600, marginBottom: 8, color: "var(--text-2)" }}>
          Number of rows
        </label>
        <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
          {QUICK.map((n) => (
            <button
              key={n}
              className="btn"
              style={{
                fontSize: 11.5,
                padding: "4px 12px",
                background: rowCount === n ? "var(--brand-cyan)" : undefined,
                color: rowCount === n ? "#000" : undefined,
                fontWeight: rowCount === n ? 700 : undefined,
              }}
              onClick={() => { setRowCount(n); setRawInput(String(n)); }}
            >
              {n}
            </button>
          ))}
          <input
            data-testid="row-count-input"
            className="conn-input"
            type="number"
            min={1}
            max={1000}
            value={rawInput}
            onChange={(e) => handleChange(e.target.value)}
            style={{ width: 80, marginLeft: 4 }}
            placeholder="Custom"
          />
        </div>
      </div>

      {/* Column info */}
      {columns.length === 0 && (
        <div style={{ fontSize: 12, color: "var(--text-3)", padding: "8px 0" }}>
          No column metadata — schema may not be loaded. Refresh schema tree and retry.
        </div>
      )}

      {/* SQL preview */}
      {columns.length > 0 && (
        <div>
          <label style={{ display: "block", fontSize: 12, fontWeight: 600, marginBottom: 6, color: "var(--text-2)" }}>
            SQL preview{rowCount > 3 ? ` (first 3 of ${rowCount} rows shown)` : ""}
          </label>
          <pre
            style={{
              background: "var(--bg-2)",
              border: "1px solid var(--border)",
              borderRadius: 6,
              padding: "10px 12px",
              fontSize: 11.5,
              maxHeight: 180,
              overflowY: "auto",
              margin: 0,
              color: "var(--text-1)",
              lineHeight: 1.6,
            }}
          >
            {previewSql}
          </pre>
        </div>
      )}

      <Footer
        onCancel={close}
        label={`Generate ${rowCount} ${rowCount === 1 ? "Row" : "Rows"} →`}
        disabled={columns.length === 0}
        onSubmit={() => {
          const n = Math.max(1, Math.min(1000, rowCount));
          onSubmit(buildSql(n), `insert_${tableName}.sql`);
        }}
      />
    </div>
  );
}
