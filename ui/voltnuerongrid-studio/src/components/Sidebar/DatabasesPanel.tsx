/**
 * DatabasesPanel — list / create / drop databases for the active connection.
 *
 * Phase 1.3. Wired to the new `/api/v1/admin/databases` endpoints.
 *
 * Design notes:
 * - All actions go through StudioApiClient (single source of truth for headers).
 * - Loading and error states are visible — never a blank panel.
 * - Drop is gated by a confirmation; the modal uses native `confirm` for now
 *   (Phase 1.5 can swap in the project's ResourceModal once that supports
 *   destructive confirmations cleanly).
 * - Names are lowercased + validated client-side mirroring the server's
 *   DatabaseName::parse rules so the user gets immediate feedback.
 */
import { useCallback, useEffect, useState } from "react";
import { StudioApiClient } from "@/api/studio-client";
import { useConnectionStore } from "@/store/connection";
import { useDatabasesStore } from "@/store/databases";
import { useToastStore } from "@/store/toast";

const NAME_PATTERN = /^[a-z_][a-z0-9_]{0,62}$/;
const RESERVED_NAMES = new Set(["metadata", "information_schema", "pg_catalog", "vng_system"]);

function validateDatabaseName(input: string): string | null {
  const trimmed = input.trim();
  if (!trimmed) return "Name cannot be empty.";
  const lower = trimmed.toLowerCase();
  if (!NAME_PATTERN.test(lower)) {
    return "Name must start with a letter or underscore and contain only letters, digits, or underscores (max 63 chars).";
  }
  if (RESERVED_NAMES.has(lower)) {
    return `Name "${lower}" is reserved.`;
  }
  return null;
}

export function DatabasesPanel() {
  const activeConn = useConnectionStore((s) => s.getActive());
  const activeKey = useConnectionStore((s) => s.getActiveKey());
  const { databases, status, error, selectedName } = useDatabasesStore();
  const { setDatabases, setStatus, selectDatabase, upsertDatabase, removeDatabase } =
    useDatabasesStore();
  const pushToast = useToastStore((s) => s.show);

  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [newDescription, setNewDescription] = useState("");
  const [newError, setNewError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const refresh = useCallback(async () => {
    if (!activeConn) {
      setStatus("idle");
      setDatabases([]);
      return;
    }
    setStatus("loading");
    try {
      const client = new StudioApiClient({
        baseUrl: activeConn.baseUrl,
        adminApiKey: activeKey,
        operatorId: activeConn.operatorId,
      });
      const resp = await client.listDatabases();
      setDatabases(resp.databases);
      setStatus("ok");
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setStatus("error", msg);
    }
  }, [activeConn, activeKey, setDatabases, setStatus]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function handleCreate() {
    const validationError = validateDatabaseName(newName);
    if (validationError) {
      setNewError(validationError);
      return;
    }
    if (!activeConn) return;
    setSubmitting(true);
    setNewError(null);
    try {
      const client = new StudioApiClient({
        baseUrl: activeConn.baseUrl,
        adminApiKey: activeKey,
        operatorId: activeConn.operatorId,
      });
      const resp = await client.createDatabase({
        name: newName.trim(),
        description: newDescription.trim() || undefined,
      });
      if (resp.database) {
        upsertDatabase(resp.database);
        pushToast(`Created database "${resp.database.name}"`, "success");
      }
      setNewName("");
      setNewDescription("");
      setCreating(false);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setNewError(msg);
    } finally {
      setSubmitting(false);
    }
  }

  async function handleDrop(name: string) {
    if (!activeConn) return;
    const confirmed = window.confirm(
      `Drop database "${name}"? This action cannot be undone.`,
    );
    if (!confirmed) return;
    try {
      const client = new StudioApiClient({
        baseUrl: activeConn.baseUrl,
        adminApiKey: activeKey,
        operatorId: activeConn.operatorId,
      });
      await client.dropDatabase(name);
      removeDatabase(name);
      pushToast(`Dropped database "${name}"`, "success");
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      pushToast(`Failed to drop "${name}": ${msg}`, "error");
    }
  }

  return (
    <div style={{ padding: "12px 10px", display: "flex", flexDirection: "column", gap: 10 }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <span className="label-xs">Databases</span>
        <button
          type="button"
          onClick={() => setCreating((c) => !c)}
          style={{
            background: "transparent",
            border: "1px solid var(--border)",
            color: "var(--text-2)",
            borderRadius: "var(--radius-sm)",
            padding: "2px 8px",
            fontSize: 11,
            cursor: "pointer",
          }}
          disabled={!activeConn}
          title={activeConn ? "New database" : "Connect first"}
        >
          {creating ? "Cancel" : "+ New"}
        </button>
      </div>

      {creating && (
        <div
          style={{
            background: "var(--bg-2)",
            border: "1px solid var(--border)",
            borderRadius: "var(--radius-md)",
            padding: 8,
            display: "flex",
            flexDirection: "column",
            gap: 6,
          }}
        >
          <input
            type="text"
            placeholder="database name (lowercase, a-z 0-9 _)"
            value={newName}
            onChange={(e) => {
              setNewName(e.target.value);
              setNewError(null);
            }}
            disabled={submitting}
            style={{
              background: "var(--bg-1)",
              border: "1px solid var(--border)",
              color: "var(--text-1)",
              borderRadius: "var(--radius-sm)",
              padding: "5px 8px",
              fontSize: 12,
            }}
          />
          <input
            type="text"
            placeholder="description (optional)"
            value={newDescription}
            onChange={(e) => setNewDescription(e.target.value)}
            disabled={submitting}
            style={{
              background: "var(--bg-1)",
              border: "1px solid var(--border)",
              color: "var(--text-1)",
              borderRadius: "var(--radius-sm)",
              padding: "5px 8px",
              fontSize: 12,
            }}
          />
          {newError && (
            <div style={{ color: "var(--red)", fontSize: 11 }}>{newError}</div>
          )}
          <button
            type="button"
            onClick={handleCreate}
            disabled={submitting || !newName.trim()}
            style={{
              background: "var(--brand-cyan)",
              color: "var(--text-inv)",
              border: "none",
              borderRadius: "var(--radius-sm)",
              padding: "5px 10px",
              fontSize: 12,
              fontWeight: 600,
              cursor: submitting ? "default" : "pointer",
              opacity: submitting ? 0.6 : 1,
            }}
          >
            {submitting ? "Creating..." : "Create"}
          </button>
        </div>
      )}

      {status === "loading" && (
        <div style={{ color: "var(--text-3)", fontSize: 11 }}>Loading databases…</div>
      )}
      {status === "error" && (
        <div style={{ color: "var(--red)", fontSize: 11 }}>{error}</div>
      )}
      {status === "ok" && databases.length === 0 && (
        <div style={{ color: "var(--text-3)", fontSize: 11 }}>
          No databases yet. Click + New to create one.
        </div>
      )}

      <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        {databases.map((db) => (
          <div
            key={db.name}
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              gap: 6,
              padding: "5px 7px",
              borderRadius: "var(--radius-sm)",
              cursor: "pointer",
              background:
                selectedName === db.name ? "var(--bg-active)" : "transparent",
              border: "1px solid transparent",
            }}
            onClick={() => selectDatabase(db.name)}
            onMouseEnter={(e) =>
              (e.currentTarget.style.background = "var(--bg-hover)")
            }
            onMouseLeave={(e) => {
              if (selectedName !== db.name) {
                e.currentTarget.style.background = "transparent";
              }
            }}
          >
            <div style={{ display: "flex", flexDirection: "column", overflow: "hidden" }}>
              <span style={{ color: "var(--text-1)", fontSize: 12 }}>{db.name}</span>
              {db.description && (
                <span
                  style={{
                    color: "var(--text-3)",
                    fontSize: 10,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                  title={db.description}
                >
                  {db.description}
                </span>
              )}
            </div>
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                void handleDrop(db.name);
              }}
              style={{
                background: "transparent",
                border: "none",
                color: "var(--text-3)",
                cursor: "pointer",
                fontSize: 12,
                padding: "2px 4px",
              }}
              title={`Drop ${db.name}`}
              aria-label={`Drop ${db.name}`}
            >
              ✕
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
