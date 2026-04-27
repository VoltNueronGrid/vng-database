// Action factories: build context-menu item arrays for each resource type.
// Side effects (modal open, store updates) are wired in via callbacks here so the
// actual menus stay declarative.

import type { ContextMenuItem } from "@/store/contextMenu";
import { useModalStore } from "@/store/modal";
import { useUiStore } from "@/store/ui";
import { useConnectionStore, type ConnectionSettings } from "@/store/connection";
import { useEditorStore } from "@/store/editor";
import type { SchemaTable, SchemaColumn } from "@/api/studio-client";

const m = () => useModalStore.getState();
const u = () => useUiStore.getState();
const c = () => useConnectionStore.getState();
const e = () => useEditorStore.getState();

// ─── Connection menu ─────────────────────────────────────────
export function buildConnectionMenu(
  conn: ConnectionSettings,
  refreshSchema: () => void
): { items: ContextMenuItem[]; title?: string } {
  const isActive = c().activeId === conn.id;
  return {
    title: conn.name,
    items: [
      {
        id: "connect",
        label: isActive ? "Reconnect" : "Connect",
        icon: "⚡",
        onSelect: () => {
          c().setActive(conn.id);
          u().setScreen("main");
          refreshSchema();
        },
      },
      {
        id: "disconnect",
        label: "Disconnect",
        icon: "⏻",
        disabled: !isActive,
        onSelect: () => {
          c().setActive(null);
          c().setSchema(null);
        },
      },
      { id: "sep1", separator: true },
      {
        id: "refresh",
        label: "Refresh Schema",
        icon: "↻",
        shortcut: "F5",
        disabled: !isActive,
        onSelect: refreshSchema,
      },
      {
        id: "test",
        label: "Test Connection",
        icon: "✓",
        onSelect: () => u().openConnectionPanel(conn.id),
      },
      { id: "sep2", separator: true },
      {
        id: "edit",
        label: "Edit Connection…",
        icon: "✎",
        onSelect: () => u().openConnectionPanel(conn.id),
      },
      {
        id: "duplicate",
        label: "Duplicate",
        icon: "⎘",
        onSelect: () => {
          const copy = {
            ...conn,
            id: `conn-${Date.now()}`,
            name: `${conn.name} (Copy)`,
            createdAt: Date.now(),
          };
          c().addConnection(copy);
        },
      },
      { id: "sep3", separator: true },
      {
        id: "newdb",
        label: "New Database…",
        icon: "＋",
        disabled: !isActive,
        onSelect: () => m().open({ kind: "create-database" }),
      },
      {
        id: "newuser",
        label: "New User…",
        icon: "👤",
        disabled: !isActive,
        onSelect: () => m().open({ kind: "create-user" }),
      },
      { id: "sep4", separator: true },
      {
        id: "remove",
        label: "Remove Connection",
        icon: "🗑",
        danger: true,
        onSelect: () => {
          if (confirm(`Remove connection "${conn.name}"?`)) {
            c().removeConnection(conn.id);
          }
        },
      },
    ],
  };
}

// ─── Database menu ───────────────────────────────────────────
export function buildDatabaseMenu(
  dbName: string
): { items: ContextMenuItem[]; title?: string } {
  return {
    title: dbName,
    items: [
      {
        id: "use",
        label: "Set as Active Database",
        icon: "★",
        onSelect: () => {
          /* hook into editor "current db" once supported */
        },
      },
      { id: "sep1", separator: true },
      {
        id: "newschema",
        label: "New Schema…",
        icon: "＋",
        onSelect: () => m().open({ kind: "create-schema", target: dbName }),
      },
      {
        id: "newtable",
        label: "New Table…",
        icon: "📋",
        onSelect: () => m().open({ kind: "create-table", target: dbName }),
      },
      { id: "sep2", separator: true },
      {
        id: "ddl",
        label: "View DDL",
        icon: "{ }",
        onSelect: () =>
          m().open({
            kind: "view-ddl",
            target: dbName,
            payload: { kind: "database" },
          }),
      },
      {
        id: "rename",
        label: "Rename…",
        icon: "✎",
        onSelect: () =>
          m().open({
            kind: "rename-table",
            target: dbName,
            payload: { kind: "database" },
          }),
      },
      { id: "sep3", separator: true },
      {
        id: "drop",
        label: "Drop Database…",
        icon: "🗑",
        danger: true,
        onSelect: () => m().open({ kind: "drop-database", target: dbName }),
      },
    ],
  };
}

// ─── Schema menu ─────────────────────────────────────────────
export function buildSchemaMenu(
  dbName: string,
  schemaName: string
): { items: ContextMenuItem[]; title?: string } {
  const target = `${dbName}.${schemaName}`;
  return {
    title: target,
    items: [
      {
        id: "newtable",
        label: "New Table…",
        icon: "＋",
        onSelect: () => m().open({ kind: "create-table", target }),
      },
      {
        id: "ddl",
        label: "View DDL",
        icon: "{ }",
        onSelect: () =>
          m().open({
            kind: "view-ddl",
            target,
            payload: { kind: "schema" },
          }),
      },
      { id: "sep1", separator: true },
      {
        id: "drop",
        label: "Drop Schema…",
        icon: "🗑",
        danger: true,
        onSelect: () => m().open({ kind: "drop-schema", target }),
      },
    ],
  };
}

// ─── Table menu ──────────────────────────────────────────────
export function buildTableMenu(
  dbName: string,
  schemaName: string,
  table: SchemaTable
): { items: ContextMenuItem[]; title?: string } {
  const target = `${dbName}.${schemaName}.${table.name}`;
  const tabKey = `${schemaName}.${table.name}`;
  return {
    title: table.name,
    items: [
      {
        id: "open",
        label: "Open Table",
        icon: "👁",
        shortcut: "Enter",
        onSelect: () => e().openTableTab(table.name, schemaName),
      },
      {
        id: "select100",
        label: "SELECT * LIMIT 100",
        icon: "⌕",
        onSelect: () => e().openTableTab(table.name, schemaName),
      },
      {
        id: "selectcount",
        label: "SELECT COUNT(*)",
        icon: "Σ",
        onSelect: () =>
          e().openSqlTab(
            `SELECT COUNT(*) FROM ${schemaName}.${table.name};`,
            `count_${table.name}.sql`
          ),
      },
      { id: "sep1", separator: true },
      {
        id: "insert",
        label: "Generate INSERT…",
        icon: "＋",
        submenu: [
          {
            id: "insert-stub",
            label: "Single row template",
            icon: "·",
            onSelect: () => {
              const cols = table.columns.map((c) => c.name).join(", ");
              const vals = table.columns.map(() => "?").join(", ");
              e().openSqlTab(
                `INSERT INTO ${schemaName}.${table.name} (${cols})\nVALUES (${vals});`,
                `insert_${table.name}.sql`
              );
            },
          },
          {
            id: "update-stub",
            label: "UPDATE template",
            icon: "·",
            onSelect: () => {
              const sets = table.columns
                .filter((c) => !c.primary_key)
                .map((c) => `  ${c.name} = ?`)
                .join(",\n");
              const wh = table.columns
                .filter((c) => c.primary_key)
                .map((c) => `${c.name} = ?`)
                .join(" AND ");
              e().openSqlTab(
                `UPDATE ${schemaName}.${table.name}\nSET\n${sets}\nWHERE ${wh || "1=1"};`,
                `update_${table.name}.sql`
              );
            },
          },
          {
            id: "delete-stub",
            label: "DELETE template",
            icon: "·",
            onSelect: () => {
              const wh = table.columns
                .filter((c) => c.primary_key)
                .map((c) => `${c.name} = ?`)
                .join(" AND ");
              e().openSqlTab(
                `DELETE FROM ${schemaName}.${table.name}\nWHERE ${wh || "1=1"};`,
                `delete_${table.name}.sql`
              );
            },
          },
        ],
      },
      { id: "sep2", separator: true },
      {
        id: "details",
        label: "Show Details",
        icon: "ℹ",
        onSelect: () => u().openRightPanel(tabKey),
      },
      {
        id: "ddl",
        label: "View DDL",
        icon: "{ }",
        onSelect: () =>
          m().open({
            kind: "view-ddl",
            target,
            payload: { kind: "table", table },
          }),
      },
      {
        id: "analyze",
        label: "Analyze Table",
        icon: "📊",
        onSelect: () =>
          e().openSqlTab(
            `ANALYZE TABLE ${schemaName}.${table.name};`,
            `analyze_${table.name}.sql`
          ),
      },
      { id: "sep3", separator: true },
      {
        id: "rename",
        label: "Rename…",
        icon: "✎",
        onSelect: () => m().open({ kind: "rename-table", target }),
      },
      {
        id: "truncate",
        label: "Truncate Table…",
        icon: "⌫",
        danger: true,
        onSelect: () => m().open({ kind: "truncate-table", target }),
      },
      {
        id: "drop",
        label: "Drop Table…",
        icon: "🗑",
        danger: true,
        onSelect: () => m().open({ kind: "drop-table", target }),
      },
    ],
  };
}

// ─── Column menu ─────────────────────────────────────────────
export function buildColumnMenu(
  dbName: string,
  schemaName: string,
  tableName: string,
  col: SchemaColumn
): { items: ContextMenuItem[]; title?: string } {
  const target = `${dbName}.${schemaName}.${tableName}.${col.name}`;
  return {
    title: `${col.name} : ${col.data_type}`,
    items: [
      {
        id: "filter",
        label: `Filter by ${col.name}`,
        icon: "⌕",
        onSelect: () =>
          e().openSqlTab(
            `SELECT *\nFROM ${schemaName}.${tableName}\nWHERE ${col.name} = ?;`,
            `where_${col.name}.sql`
          ),
      },
      {
        id: "groupby",
        label: `GROUP BY ${col.name}`,
        icon: "⌗",
        onSelect: () =>
          e().openSqlTab(
            `SELECT ${col.name}, COUNT(*)\nFROM ${schemaName}.${tableName}\nGROUP BY ${col.name};`,
            `groupby_${col.name}.sql`
          ),
      },
      { id: "sep1", separator: true },
      {
        id: "edit",
        label: "Edit Column…",
        icon: "✎",
        onSelect: () =>
          m().open({ kind: "edit-column", target, payload: { col } }),
      },
      {
        id: "drop",
        label: "Drop Column…",
        icon: "🗑",
        danger: true,
        onSelect: () =>
          m().open({ kind: "drop-column", target, payload: { col } }),
      },
    ],
  };
}

// ─── User menu ───────────────────────────────────────────────
export function buildUserMenu(
  username: string
): { items: ContextMenuItem[]; title?: string } {
  return {
    title: username,
    items: [
      {
        id: "edit",
        label: "Edit User…",
        icon: "✎",
        onSelect: () =>
          m().open({ kind: "create-user", target: username, payload: { edit: true } }),
      },
      {
        id: "grant",
        label: "Grant Role…",
        icon: "🛡",
        onSelect: () => m().open({ kind: "grant-role", target: username }),
      },
      {
        id: "resetpw",
        label: "Reset Password…",
        icon: "🔑",
        onSelect: () =>
          e().openSqlTab(
            `ALTER USER ${username} WITH PASSWORD '<new-password>';`,
            `reset_${username}.sql`
          ),
      },
      { id: "sep1", separator: true },
      {
        id: "drop",
        label: "Drop User…",
        icon: "🗑",
        danger: true,
        onSelect: () => m().open({ kind: "drop-user", target: username }),
      },
    ],
  };
}
