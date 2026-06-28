/**
 * R9: DatabaseChoiceModal — shown when a connection's target database is not found.
 *
 * The user can:
 *  1. Select an existing database from the server list.
 *  2. Create a new empty database with the configured name.
 *  3. Cancel — connection stays in `awaiting_db_choice` state (workspace stays hidden).
 */
import { useState, useEffect } from "react";
import { useConnectionStore } from "@/store/connection";
import { StudioApiClient } from "@/api/studio-client";

export function DatabaseChoiceModal() {
  const lifecycleState = useConnectionStore((s) => s.lifecycleState);
  const activeId = useConnectionStore((s) => s.activeId);
  const connections = useConnectionStore((s) => s.connections);
  const resolvedKeys = useConnectionStore((s) => s.resolvedKeys);
  const confirmDatabase = useConnectionStore((s) => s.confirmDatabase);
  const setActive = useConnectionStore((s) => s.setActive);

  const [existing, setExisting] = useState<string[]>([]);
  const [selected, setSelected] = useState<string>("");
  const [newDbName, setNewDbName] = useState<string>("");
  const [mode, setMode] = useState<"pick" | "create">("pick");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const conn = activeId ? connections.find((c) => c.id === activeId) : null;

  useEffect(() => {
    if (lifecycleState !== "awaiting_db_choice" || !conn) return;
    const adminKey = resolvedKeys[conn.id] ?? conn.adminKey;
    const client = new StudioApiClient({
      baseUrl: conn.baseUrl,
      adminApiKey: conn.mode === "admin" ? adminKey : undefined,
      operatorId: conn.operatorId,
    });
    client
      .listDatabases()
      .then((res) => {
        const names = res.databases.map((d) => d.name);
        setExisting(names);
        if (names.length > 0) setSelected(names[0]);
        // Pre-fill the create field with the configured database name if set.
        setNewDbName(conn.database ?? "");
      })
      .catch(() => setExisting([]));
  }, [lifecycleState, conn, resolvedKeys]);

  if (lifecycleState !== "awaiting_db_choice") return null;

  async function handlePickExisting() {
    if (!selected) return;
    confirmDatabase(selected);
  }

  async function handleCreateNew() {
    if (!newDbName.trim() || !conn) return;
    setBusy(true);
    setError(null);
    const adminKey = resolvedKeys[conn.id] ?? conn.adminKey;
    const client = new StudioApiClient({
      baseUrl: conn.baseUrl,
      adminApiKey: conn.mode === "admin" ? adminKey : undefined,
      operatorId: conn.operatorId,
    });
    try {
      await client.createDatabase({ name: newDbName.trim(), if_not_exists: true });
      confirmDatabase(newDbName.trim());
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  function handleCancel() {
    // Reset connection to idle — workspace stays gated.
    setActive(null);
  }

  return (
    <div className="overlay">
      <div className="conn-panel" style={{ maxWidth: 460 }}>
        <div className="conn-panel-header">
          <div className="logo-icon" style={{ width: 28, height: 28, fontSize: 14 }}>V</div>
          <span className="conn-panel-title">Database Not Found</span>
          <button className="conn-panel-close" onClick={handleCancel}>✕</button>
        </div>

        <div className="conn-panel-body" style={{ padding: "18px 20px" }}>
          <p style={{ color: "var(--text-2)", marginBottom: 16, lineHeight: 1.5 }}>
            The database <strong style={{ color: "var(--text-1)" }}>
              {conn?.database ?? "(unset)"}
            </strong> was not found on this server. Choose how to proceed:
          </p>

          <div style={{ display: "flex", gap: 8, marginBottom: 18 }}>
            <button
              className={`cp-tab${mode === "pick" ? " active" : ""}`}
              onClick={() => setMode("pick")}
              style={{ flex: 1 }}
            >
              Select existing
            </button>
            <button
              className={`cp-tab${mode === "create" ? " active" : ""}`}
              onClick={() => setMode("create")}
              style={{ flex: 1 }}
            >
              Create new
            </button>
          </div>

          {mode === "pick" && (
            <>
              {existing.length === 0 ? (
                <p style={{ color: "var(--text-3)", fontSize: 12 }}>
                  No databases found on this server.
                </p>
              ) : (
                <div className="form-field full" style={{ marginBottom: 14 }}>
                  <label className="form-label">Available databases</label>
                  <select
                    className="form-select"
                    value={selected}
                    onChange={(e) => setSelected(e.target.value)}
                  >
                    {existing.map((name) => (
                      <option key={name} value={name}>{name}</option>
                    ))}
                  </select>
                </div>
              )}
              <div className="conn-panel-footer" style={{ justifyContent: "flex-end", marginTop: 8 }}>
                <button className="btn-secondary" onClick={handleCancel}>Cancel</button>
                <button
                  className="btn-primary"
                  onClick={handlePickExisting}
                  disabled={!selected}
                >
                  Connect to {selected || "…"}
                </button>
              </div>
            </>
          )}

          {mode === "create" && (
            <>
              <div className="form-field full" style={{ marginBottom: 14 }}>
                <label className="form-label">New database name</label>
                <input
                  className="form-input"
                  value={newDbName}
                  onChange={(e) => setNewDbName(e.target.value)}
                  placeholder="e.g. myapp_dev"
                />
              </div>
              {error && (
                <p style={{ color: "var(--red)", fontSize: 12, marginBottom: 10 }}>{error}</p>
              )}
              <div className="conn-panel-footer" style={{ justifyContent: "flex-end", marginTop: 8 }}>
                <button className="btn-secondary" onClick={handleCancel}>Cancel</button>
                <button
                  className="btn-primary"
                  onClick={handleCreateNew}
                  disabled={busy || !newDbName.trim()}
                >
                  {busy ? "Creating…" : "Create & Connect"}
                </button>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
