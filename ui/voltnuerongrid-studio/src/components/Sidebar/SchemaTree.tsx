import { useState, useCallback } from "react";
import { useConnectionStore } from "@/store/connection";
import { useUiStore } from "@/store/ui";
import { useEditorStore } from "@/store/editor";
import { useSettingsStore } from "@/store/settings";
import { useToastStore } from "@/store/toast";
import { openMenuFor } from "@/store/contextMenu";
import {
  buildDatabaseMenu,
  buildSchemaMenu,
  buildTablesSectionMenu,
  buildViewsSectionMenu,
  buildFunctionsSectionMenu,
  buildTriggersSectionMenu,
  buildEventsSectionMenu,
  buildTableMenu,
  buildViewMenu,
  buildFunctionMenu,
  buildTriggerMenu,
  buildEventMenu,
  buildColumnMenu,
} from "@/components/ContextMenu/menus";
import type {
  SchemaDatabase,
  SchemaNamespace,
  SchemaTable,
  SchemaFunction,
  SchemaView,
  SchemaTrigger,
  SchemaEvent,
} from "@/api/studio-client";

function TreeIndents({ count }: { count: number }) {
  return Array.from({ length: count }, (_value, index) => (
    <span key={index} className="tree-indent" />
  ));
}

/**
 * Returns a stable callback that, given an object name and its DDL definition
 * string, performs the action configured in Studio Settings:
 *   • "open_tab"       → open a new unsaved SQL tab containing the DDL
 *   • "copy_clipboard" → copy the DDL to the clipboard and show a toast
 */
function useDdlAction() {
  const action = useSettingsStore((s) => s.ddlDoubleClickAction);
  const openSqlTab = useEditorStore((s) => s.openSqlTab);
  const showToast = useToastStore((s) => s.show);

  return useCallback(
    (name: string, definition: string | undefined) => {
      const ddl = definition?.trim() || `-- No DDL available for: ${name}`;
      if (action === "copy_clipboard") {
        navigator.clipboard.writeText(ddl).then(
          () => showToast(`DDL for "${name}" copied to clipboard`, "success"),
          () => showToast("Failed to copy to clipboard", "error"),
        );
      } else {
        // Default: open_tab
        openSqlTab(ddl, `${name}.sql`);
      }
    },
    [action, openSqlTab, showToast],
  );
}

function colTypeClass(type: string): string {
  const t = type.toUpperCase();
  if (t.includes("INT") || t.includes("FLOAT") || t.includes("DECIMAL") || t.includes("NUM"))
    return "int";
  if (t.includes("BOOL")) return "bool";
  if (t.includes("DATE") || t.includes("TIME")) return "date";
  if (t.includes("JSON")) return "json";
  return "str";
}

function TableNode({
  table,
  schemaName,
  dbName,
  indentLevel = 4,
}: {
  table: SchemaTable;
  schemaName: string;
  dbName: string;
  indentLevel?: number;
}) {
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
        <TreeIndents count={indentLevel} />
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
          <TreeIndents count={indentLevel + 1} />
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

function FunctionNode({
  fn: func,
  dbName,
  schemaName,
  indentLevel = 4,
}: {
  fn: SchemaFunction;
  dbName: string;
  schemaName: string;
  indentLevel?: number;
}) {
  const [open, setOpen] = useState(false);
  const onDdlAction = useDdlAction();
  const actionLabel = useSettingsStore((s) =>
    s.ddlDoubleClickAction === "copy_clipboard"
      ? "Double-click to copy DDL"
      : "Double-click to open DDL"
  );

  return (
    <>
      <div
        className="tree-node"
        style={{ paddingLeft: 0 }}
        onClick={() => setOpen((o) => !o)}
        onDoubleClick={() => onDdlAction(func.name, func.definition)}
        title={actionLabel}
        onContextMenu={openMenuFor(() => buildFunctionMenu(dbName, schemaName, func))}
      >
        <TreeIndents count={indentLevel} />
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
            <TreeIndents count={indentLevel + 1} />
            <span className="tree-chevron" style={{ visibility: "hidden" }}>▶</span>
            <span style={{ width: 14 }} />
            <span className="tree-label mono" style={{ fontSize: 11, color: "var(--text-2)" }}>({func.arguments || "void"})</span>
            <span className="col-chip str" style={{ marginLeft: 4 }}>args</span>
          </div>
          {/* Return type */}
          <div className="tree-node" style={{ paddingLeft: 0 }}>
            <TreeIndents count={indentLevel + 1} />
            <span className="tree-chevron" style={{ visibility: "hidden" }}>▶</span>
            <span style={{ width: 14 }} />
            <span className="tree-label mono" style={{ fontSize: 11, color: "var(--text-2)" }}>→ {func.return_type}</span>
            <span className="col-chip str" style={{ marginLeft: 4 }}>returns</span>
          </div>
          {/* Language */}
          <div className="tree-node" style={{ paddingLeft: 0 }}>
            <TreeIndents count={indentLevel + 1} />
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

function NamedObjectNode({
  icon,
  name,
  dbName,
  schemaName,
  badge,
  definition,
  menuBuilder,
  indentLevel = 4,
}: {
  icon: string;
  name: string;
  dbName: string;
  schemaName: string;
  badge?: string;
  definition?: string;
  menuBuilder?: (dbName: string, schemaName: string, name: string, definition?: string) => { items: import("@/store/contextMenu").ContextMenuItem[]; title?: string };
  indentLevel?: number;
}) {
  const onDdlAction = useDdlAction();
  const actionLabel = useSettingsStore((s) =>
    s.ddlDoubleClickAction === "copy_clipboard"
      ? "Double-click to copy DDL"
      : "Double-click to open DDL"
  );

  return (
    <div
      className="tree-node"
      style={{ paddingLeft: 0 }}
      title={actionLabel}
      onDoubleClick={() => onDdlAction(name, definition)}
      onContextMenu={
        menuBuilder
          ? openMenuFor(() => menuBuilder(dbName, schemaName, name, definition))
          : undefined
      }
    >
      <TreeIndents count={indentLevel} />
      <span className="tree-chevron" style={{ visibility: "hidden" }}>▶</span>
      <span className="tree-icon">{icon}</span>
      <span className="tree-label">{name}</span>
      {badge && (
        <span className="col-chip" style={{ fontSize: 10, padding: "1px 5px", borderRadius: 4, marginLeft: 4 }}>
          {badge}
        </span>
      )}
    </div>
  );
}

function SectionNode({
  icon,
  label,
  count,
  indentLevel,
  defaultOpen = true,
  onContextMenu,
  children,
}: {
  icon: string;
  label: string;
  count: number;
  indentLevel: number;
  defaultOpen?: boolean;
  onContextMenu?: React.MouseEventHandler<HTMLDivElement>;
  children?: React.ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <>
      <div
        className="tree-node"
        style={{ paddingLeft: 0, opacity: 0.75 }}
        onClick={() => setOpen((value) => !value)}
        onContextMenu={onContextMenu}
      >
        <TreeIndents count={indentLevel} />
        <span className={`tree-chevron ${open ? "open" : ""}`}>▶</span>
        <span className="tree-icon">{icon}</span>
        <span className="tree-label" style={{ fontSize: 10, textTransform: "uppercase", letterSpacing: 1 }}>
          {label}
        </span>
        <span className="tree-count" style={{ fontSize: 10 }}>{count}</span>
      </div>
      {open && children}
    </>
  );
}

function SchemaNode({ ns, dbName }: { ns: SchemaNamespace; dbName: string }) {
  const [open, setOpen] = useState(true);
  const views = ns.views ?? [];
  const functions = ns.functions ?? [];
  const triggers = ns.triggers ?? [];
  const events = ns.events ?? [];
  const totalItems = ns.tables.length + views.length + functions.length + triggers.length + events.length;

  return (
    <>
      <div
        className="tree-node"
        onClick={() => setOpen((o) => !o)}
        onContextMenu={openMenuFor(() => buildSchemaMenu(dbName, ns.name))}
      >
        <TreeIndents count={3} />
        <span className={`tree-chevron ${open ? "open" : ""}`}>▶</span>
        <span className="tree-icon">📁</span>
        <span className="tree-label">{ns.name}</span>
        <span className="tree-count">{totalItems}</span>
      </div>

      {open && (
        <>
          <SectionNode
            icon="📋"
            label="Tables"
            count={ns.tables.length}
            indentLevel={4}
            onContextMenu={openMenuFor(() => buildTablesSectionMenu(dbName, ns.name))}
          >
            {ns.tables.map((table) => (
              <TableNode key={table.name} table={table} schemaName={ns.name} dbName={dbName} indentLevel={5} />
            ))}
          </SectionNode>

          <SectionNode
            icon="👁"
            label="Views"
            count={views.length}
            indentLevel={4}
            defaultOpen={false}
            onContextMenu={openMenuFor(() => buildViewsSectionMenu(dbName, ns.name))}
          >
            {views.map((view: SchemaView) => (
              <NamedObjectNode
                key={view.name}
                icon="👁"
                name={view.name}
                dbName={dbName}
                schemaName={ns.name}
                badge="view"
                definition={view.definition}
                menuBuilder={buildViewMenu}
                indentLevel={5}
              />
            ))}
          </SectionNode>

          <SectionNode
            icon="⚡"
            label="Functions"
            count={functions.length}
            indentLevel={4}
            onContextMenu={openMenuFor(() => buildFunctionsSectionMenu(dbName, ns.name))}
          >
            {functions.map((func) => (
              <FunctionNode key={func.name} fn={func} dbName={dbName} schemaName={ns.name} indentLevel={5} />
            ))}
          </SectionNode>

          <SectionNode
            icon="⛓"
            label="Triggers"
            count={triggers.length}
            indentLevel={4}
            defaultOpen={false}
            onContextMenu={openMenuFor(() => buildTriggersSectionMenu(dbName, ns.name))}
          >
            {triggers.map((trigger: SchemaTrigger) => (
              <NamedObjectNode
                key={trigger.name}
                icon="⛓"
                name={trigger.name}
                dbName={dbName}
                schemaName={ns.name}
                badge="trigger"
                definition={trigger.definition}
                menuBuilder={buildTriggerMenu}
                indentLevel={5}
              />
            ))}
          </SectionNode>

          <SectionNode
            icon="🗓"
            label="Events"
            count={events.length}
            indentLevel={4}
            defaultOpen={false}
            onContextMenu={openMenuFor(() => buildEventsSectionMenu(dbName, ns.name))}
          >
            {events.map((event: SchemaEvent) => (
              <NamedObjectNode
                key={event.name}
                icon="🗓"
                name={event.name}
                dbName={dbName}
                schemaName={ns.name}
                badge="event"
                definition={event.definition}
                menuBuilder={buildEventMenu}
                indentLevel={5}
              />
            ))}
          </SectionNode>
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
        <TreeIndents count={1} />
        <span className={`tree-chevron ${open ? "open" : ""}`}>▶</span>
        <span className="tree-icon">🗄</span>
        <span className="tree-label">{db.name}</span>
        <span className="tree-badge">{db.schemas.length} schemas</span>
      </div>
      {open && (
        <SectionNode icon="🗂" label="Schemas" count={db.schemas.length} indentLevel={2}>
          {db.schemas.map((ns) => (
            <SchemaNode key={ns.name} ns={ns} dbName={db.name} />
          ))}
        </SectionNode>
      )}
    </>
  );
}

export function SchemaTree({ embedded = false }: { embedded?: boolean }) {
  const activeId = useConnectionStore((s) => s.activeId);
  const schema = useConnectionStore((s) => s.schema);
  const getDatabases = useConnectionStore((s) => s.getDatabases);
  const databases = getDatabases();

  if (!activeId) {
    if (embedded) return null;
    return (
      <div style={{ padding: "16px 12px", color: "var(--text-3)", fontSize: 12 }}>
        Connect to a server to browse schema.
      </div>
    );
  }

  if (!schema) {
    return (
      <div className={embedded ? "embedded-schema-empty" : undefined} style={{ padding: "16px 12px", color: "var(--text-3)", fontSize: 12 }}>
        {embedded ? "Loading catalog..." : "Connect to a server to browse schema."}
      </div>
    );
  }

  if (databases.length === 0) {
    return (
      <div className={embedded ? "embedded-schema-empty" : undefined} style={{ padding: "16px 12px", color: "var(--text-3)", fontSize: 12 }}>
        {embedded ? "No databases found for this connection." : "No databases found."}
      </div>
    );
  }

  return (
    <div className={embedded ? "embedded-schema-tree" : undefined}>
      <SectionNode icon="🗄" label="Databases" count={databases.length} indentLevel={0}>
        {databases.map((db) => (
          <DatabaseNode key={db.name} db={db} />
        ))}
      </SectionNode>
    </div>
  );
}
