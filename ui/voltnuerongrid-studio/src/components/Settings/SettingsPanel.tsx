/**
 * Studio-wide Settings panel.
 *
 * Opened from the ⚙ button in the TitleBar. Persists preferences via
 * useSettingsStore (localStorage).  All settings are user-scoped — they apply
 * across every database and connection for this browser / desktop user.
 *
 * M-6: Added Connection Defaults section (isolation level, statement timeout)
 * and Server Config section (fetches /api/v1/admin/runtime-config).
 */
import { useEffect, useRef, useState } from "react";
import {
  useSettingsStore,
  type DdlDoubleClickAction,
  type IsolationLevel,
} from "@/store/settings";
import { useUiStore } from "@/store/ui";
import { useConnectionStore } from "@/store/connection";
import { StudioApiClient } from "@/api/studio-client";
import type { RuntimeConfigResponse } from "@/api/studio-client";

// ─── Server Config sub-panel ──────────────────────────────────────────────────

function ServerConfigSection() {
  const getActive = useConnectionStore((s) => s.getActive);
  const getActiveKey = useConnectionStore((s) => s.getActiveKey);
  const [cfg, setCfg] = useState<RuntimeConfigResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    const conn = getActive();
    if (!conn) return;
    setLoading(true);
    setErr(null);
    const client = new StudioApiClient({
      baseUrl: conn.baseUrl,
      adminApiKey: conn.mode === "admin" ? getActiveKey() : undefined,
      operatorId: conn.operatorId,
      tenantId: conn.tenantId,
      userId: conn.userId,
    });
    client
      .getRuntimeConfig()
      .then(setCfg)
      .catch((e: unknown) => setErr(String(e)))
      .finally(() => setLoading(false));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []); // intentionally runs only once on panel mount

  if (loading) return <p className="settings-hint">Loading server config…</p>;
  if (err) return <p className="settings-hint" style={{ color: "var(--color-error, #f04)" }}>Could not load server config: {err}</p>;
  if (!cfg) return <p className="settings-hint">No active connection.</p>;

  return (
    <dl className="settings-dl">
      <dt>Storage engine</dt><dd>{cfg.storage.engine}</dd>
      <dt>Data directory</dt><dd><code>{cfg.storage.data_dir}</code></dd>
      <dt>Background jobs</dt><dd>{cfg.storage.max_background_jobs}</dd>
      <dt>WAL fsync on commit</dt><dd>{cfg.storage.wal_fsync_on_commit ? "yes" : "no"}</dd>
      <dt>SQL engine</dt><dd>{cfg.sql.engine}</dd>
      <dt>HTAP OLAP threshold</dt>
      <dd>{cfg.sql.htap_olap_threshold_rows.toLocaleString()} rows</dd>
      <dt>Max result rows</dt>
      <dd>{cfg.sql.max_result_rows.toLocaleString()}</dd>
    </dl>
  );
}

// ─── Main panel ───────────────────────────────────────────────────────────────

export function SettingsPanel() {
  const closeSettings = useUiStore((s) => s.closeSettings);
  const {
    ddlDoubleClickAction,
    defaultQueryLimit,
    confirmUnsavedClose,
    defaultIsolationLevel,
    statementTimeoutMs,
    update,
    reset,
  } = useSettingsStore();

  const panelRef = useRef<HTMLDivElement>(null);

  // Close on Escape key
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeSettings();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [closeSettings]);

  // Close on backdrop click
  function handleBackdrop(e: React.MouseEvent) {
    if (panelRef.current && !panelRef.current.contains(e.target as Node)) {
      closeSettings();
    }
  }

  return (
    <div className="settings-overlay" onMouseDown={handleBackdrop}>
      <div className="settings-panel" ref={panelRef} role="dialog" aria-label="Studio Settings">
        {/* Header */}
        <div className="settings-header">
          <span className="settings-title">⚙ Studio Settings</span>
          <button className="settings-close" onClick={closeSettings} aria-label="Close settings">
            ✕
          </button>
        </div>

        <div className="settings-body">
          {/* ── Schema Explorer ─────────────────────────── */}
          <section className="settings-section">
            <h3 className="settings-section-title">Schema Explorer</h3>

            <div className="settings-row">
              <label className="settings-label" htmlFor="ddl-dblclick">
                Double-click on object
                <span className="settings-hint">
                  What happens when you double-click a view, trigger, function,
                  event, or other schema object in the sidebar.
                </span>
              </label>
              <select
                id="ddl-dblclick"
                className="settings-select"
                value={ddlDoubleClickAction}
                onChange={(e) =>
                  update({ ddlDoubleClickAction: e.target.value as DdlDoubleClickAction })
                }
              >
                <option value="open_tab">Open DDL in new SQL tab</option>
                <option value="copy_clipboard">Copy DDL to clipboard</option>
              </select>
            </div>
          </section>

          {/* ── Query Editor ────────────────────────────── */}
          <section className="settings-section">
            <h3 className="settings-section-title">Query Editor</h3>

            <div className="settings-row">
              <label className="settings-label" htmlFor="default-limit">
                Default row limit
                <span className="settings-hint">
                  Maximum rows returned when a query has no explicit LIMIT clause.
                </span>
              </label>
              <input
                id="default-limit"
                type="number"
                className="settings-input"
                min={1}
                max={100000}
                step={100}
                value={defaultQueryLimit}
                onChange={(e) => {
                  const v = parseInt(e.target.value, 10);
                  if (!isNaN(v) && v > 0) update({ defaultQueryLimit: v });
                }}
              />
            </div>

            <div className="settings-row">
              <label className="settings-label" htmlFor="confirm-close">
                Confirm before closing unsaved tabs
              </label>
              <input
                id="confirm-close"
                type="checkbox"
                className="settings-checkbox"
                checked={confirmUnsavedClose}
                onChange={(e) => update({ confirmUnsavedClose: e.target.checked })}
              />
            </div>
          </section>

          {/* ── M-6: Connection Defaults ─────────────────── */}
          <section className="settings-section">
            <h3 className="settings-section-title">Connection Defaults</h3>
            <p className="settings-hint" style={{ marginBottom: "0.75rem" }}>
              These values are sent with every SQL execute request and apply to
              all connections in this Studio instance.
            </p>

            <div className="settings-row">
              <label className="settings-label" htmlFor="isolation-level">
                Default isolation level
                <span className="settings-hint">
                  ACID isolation level for transactions. Higher levels increase
                  consistency but may cause more aborts under concurrent load.
                </span>
              </label>
              <select
                id="isolation-level"
                className="settings-select"
                value={defaultIsolationLevel}
                onChange={(e) =>
                  update({ defaultIsolationLevel: e.target.value as IsolationLevel })
                }
              >
                <option value="read_committed">Read Committed (default)</option>
                <option value="repeatable_read">Repeatable Read</option>
                <option value="serializable">Serializable</option>
              </select>
            </div>

            <div className="settings-row">
              <label className="settings-label" htmlFor="stmt-timeout">
                Statement timeout (ms)
                <span className="settings-hint">
                  Client-side hint sent to the server. 0 means no timeout limit.
                </span>
              </label>
              <input
                id="stmt-timeout"
                type="number"
                className="settings-input"
                min={0}
                max={3600000}
                step={1000}
                value={statementTimeoutMs}
                onChange={(e) => {
                  const v = parseInt(e.target.value, 10);
                  if (!isNaN(v) && v >= 0) update({ statementTimeoutMs: v });
                }}
              />
            </div>
          </section>

          {/* ── M-6: Server Config (read-only) ──────────── */}
          <section className="settings-section">
            <h3 className="settings-section-title">Server Configuration</h3>
            <p className="settings-hint" style={{ marginBottom: "0.75rem" }}>
              Boot-time configuration reported by the connected VoltNueronGrid node.
              Read-only — restart the server to change these values.
            </p>
            <ServerConfigSection />
          </section>
        </div>

        {/* Footer */}
        <div className="settings-footer">
          <button className="btn" onClick={reset} title="Restore all defaults">
            Reset to defaults
          </button>
          <button className="btn primary" onClick={closeSettings}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
