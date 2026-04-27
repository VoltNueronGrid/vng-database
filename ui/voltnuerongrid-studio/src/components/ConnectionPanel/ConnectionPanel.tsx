import { useState, useEffect } from "react";
import { useUiStore } from "@/store/ui";
import { useConnectionStore } from "@/store/connection";
import { defaultConnection, PROTOCOL_DEFAULT_PORTS } from "@/store/connection";
import type { ConnectionSettings, ConnectionMode, ServerType, RuntimeTarget, ConnectionProtocol } from "@/store/connection";
import { StudioApiClient } from "@/api/studio-client";
import { tauriCredentials } from "@/api/tauri";

type Tab = "general" | "auth" | "ssl" | "advanced";
type TestState = "idle" | "testing" | "ok" | "fail" | "warn";

export function ConnectionPanel() {
  const closeConnectionPanel = useUiStore((s) => s.closeConnectionPanel);
  const editingId = useUiStore((s) => s.editingConnectionId);
  const setActive = useConnectionStore((s) => s.setActive);
  const setScreen = useUiStore((s) => s.setScreen);

  const connections = useConnectionStore((s) => s.connections);
  const addConnection = useConnectionStore((s) => s.addConnection);
  const updateConnection = useConnectionStore((s) => s.updateConnection);
  const setResolvedKey = useConnectionStore((s) => s.setResolvedKey);

  const existing = editingId ? connections.find((c) => c.id === editingId) : null;

  const [form, setForm] = useState<ConnectionSettings>(
    () => existing ?? defaultConnection()
  );
  const [activeTab, setActiveTab] = useState<Tab>("general");
  const [adminKey, setAdminKey] = useState("");
  const [testState, setTestState] = useState<TestState>("idle");
  const [testMsg, setTestMsg] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (existing) {
      setForm(existing);
      // Load key from keychain if editing
      tauriCredentials.get(existing.id, "adminKey")
        .then((k) => { if (k) setAdminKey(k); })
        .catch(() => {});
    }
  }, [existing]);

  function patch<K extends keyof ConnectionSettings>(k: K, v: ConnectionSettings[K]) {
    setForm((f) => {
      const next = { ...f, [k]: v };

      // Auto-sync baseUrl when host/port/protocol change
      if (k === "host" || k === "port" || k === "protocol") {
        const host = k === "host" ? (v as string) : f.host;
        const port = k === "port" ? (v as number) : f.port;
        next.baseUrl = `http://${host}:${port}`;
      }

      // When protocol changes, auto-update port to the protocol default
      // ONLY if the current port matches the other protocol's default (i.e. not customised)
      if (k === "protocol") {
        const newProto = v as ConnectionProtocol;
        const oldDefault = PROTOCOL_DEFAULT_PORTS[f.protocol];
        if (f.port === oldDefault) {
          next.port = PROTOCOL_DEFAULT_PORTS[newProto];
          next.baseUrl = `http://${f.host}:${next.port}`;
        }
      }

      return next;
    });
  }

  /** Returns a validation error string, or null if all required fields are present. */
  function validate(): string | null {
    if (!form.host.trim()) return "Host is required";
    if (!form.port || form.port <= 0) return "Port is required";
    if (form.mode === "admin" && !adminKey.trim()) return "Admin API Key is required (Auth tab)";
    if (form.mode === "operator" && !form.operatorId?.trim()) return "Operator ID is required (Auth tab)";
    if (form.mode === "tenant" && !form.tenantId?.trim()) return "Tenant ID is required (Auth tab)";
    if (form.mode === "tenant" && !form.userId?.trim()) return "User ID is required (Auth tab)";
    return null;
  }

  async function testConnection() {
    // Validate required fields first — don't hit the network if they're missing
    const validationErr = validate();
    if (validationErr) {
      setTestState("fail");
      setTestMsg(validationErr);
      return;
    }

    // Native protocol: can't test raw TCP from the browser — validate fields only
    if (form.protocol === "native") {
      setTestState("warn");
      setTestMsg("Fields valid — native wire protocol cannot be tested from the browser");
      return;
    }

    setTestState("testing");
    setTestMsg("Connecting…");
    const client = new StudioApiClient({
      baseUrl: form.baseUrl,
      adminApiKey: form.mode === "admin" ? adminKey : undefined,
      operatorId: form.operatorId,
    });
    const start = Date.now();
    try {
      await client.health();
      setTestState("ok");
      setTestMsg(`Connected (${Date.now() - start} ms)`);
    } catch (e) {
      setTestState("fail");
      setTestMsg(String(e));
    }
  }

  async function save() {
    if (!form.name.trim()) { setError("Connection name is required"); return; }
    const validationErr = validate();
    if (validationErr) { setError(validationErr); return; }
    setError(null);

    if (existing) {
      updateConnection(form.id, form);
    } else {
      addConnection(form);
    }

    // Persist admin key in OS keychain
    if (form.mode === "admin" && adminKey) {
      tauriCredentials.store(form.id, "adminKey", adminKey).catch(() => {});
      setResolvedKey(form.id, adminKey);
    }

    setActive(form.id);
    setScreen("main");
    closeConnectionPanel();
  }

  const testStatusClass =
    testState === "ok" ? "ok"
    : testState === "fail" ? "fail"
    : testState === "warn" ? "warn"
    : testState === "testing" ? "testing" : "";

  const testStatusIcon =
    testState === "ok" ? "✔"
    : testState === "fail" ? "✗"
    : testState === "warn" ? "⚠"
    : testState === "testing" ? "⟳" : "";

  return (
    <div className="overlay" onClick={(e) => e.target === e.currentTarget && closeConnectionPanel()}>
      <div className="conn-panel">
        {/* Header */}
        <div className="conn-panel-header">
          <div className="logo-icon" style={{ width: 28, height: 28, fontSize: 14 }}>V</div>
          <span className="conn-panel-title">
            {existing ? "Edit Connection" : "New Connection"}
          </span>
          <button className="conn-panel-close" onClick={closeConnectionPanel}>✕</button>
        </div>

        {/* Tabs */}
        <div className="conn-panel-tabs">
          {(["general", "auth", "ssl", "advanced"] as Tab[]).map((t) => (
            <button
              key={t}
              className={`cp-tab ${activeTab === t ? "active" : ""}`}
              onClick={() => setActiveTab(t)}
            >
              {t.charAt(0).toUpperCase() + t.slice(1)}
            </button>
          ))}
        </div>

        {/* Body */}
        <div className="conn-panel-body">
          {activeTab === "general" && (
            <>
              <div className="form-row">
                <div className="form-field full">
                  <label className="form-label">Connection Name</label>
                  <input
                    className={`form-input${error && !form.name.trim() ? " error" : ""}`}
                    value={form.name}
                    onChange={(e) => patch("name", e.target.value)}
                    placeholder="e.g. Local Dev"
                  />
                </div>
              </div>
              <div className="form-row">
                <div className="form-field">
                  <label className="form-label">Server Type</label>
                  <select
                    className="form-select"
                    value={form.serverType}
                    onChange={(e) => patch("serverType", e.target.value as ServerType)}
                  >
                    <option value="voltnuerongrid">VoltNueronGrid</option>
                    <option value="postgresql">PostgreSQL</option>
                    <option value="mysql">MySQL</option>
                    <option value="other">Other</option>
                  </select>
                </div>
                <div className="form-field">
                  <label className="form-label">Runtime Target</label>
                  <select
                    className="form-select"
                    value={form.runtimeTarget}
                    onChange={(e) => patch("runtimeTarget", e.target.value as RuntimeTarget)}
                  >
                    <option value="local">Local</option>
                    <option value="docker">Docker</option>
                    <option value="cloud">Cloud</option>
                    <option value="custom">Custom</option>
                  </select>
                </div>
              </div>
              <div className="form-row">
                <div className="form-field full">
                  <label className="form-label">Connection Protocol</label>
                  <div className="protocol-toggle">
                    {(["http", "native"] as ConnectionProtocol[]).map((p) => (
                      <button
                        key={p}
                        className={`protocol-btn${form.protocol === p ? " selected" : ""}`}
                        onClick={() => patch("protocol", p)}
                        type="button"
                      >
                        {p === "http" ? (
                          <><span className="proto-icon">🌐</span><span className="proto-label">HTTP</span><span className="proto-desc">REST API · port {PROTOCOL_DEFAULT_PORTS.http}</span></>
                        ) : (
                          <><span className="proto-icon">⚡</span><span className="proto-label">Native</span><span className="proto-desc">Wire protocol · port {PROTOCOL_DEFAULT_PORTS.native}</span></>
                        )}
                      </button>
                    ))}
                  </div>
                </div>
              </div>
              <div className="form-row">
                <div className="form-field">
                  <label className="form-label">Host</label>
                  <input
                    className={`form-input${error && !form.host.trim() ? " error" : ""}`}
                    value={form.host}
                    onChange={(e) => patch("host", e.target.value)}
                    placeholder="127.0.0.1"
                  />
                </div>
                <div className="form-field">
                  <label className="form-label">Port</label>
                  <input
                    className="form-input"
                    type="number"
                    value={form.port}
                    onChange={(e) => patch("port", Number(e.target.value))}
                    placeholder="8080"
                  />
                </div>
              </div>
              <div className="form-row">
                <div className="form-field full">
                  <label className="form-label">Database (optional)</label>
                  <input
                    className="form-input"
                    value={form.database ?? ""}
                    onChange={(e) => patch("database", e.target.value || undefined)}
                    placeholder="Leave blank to browse all"
                  />
                </div>
              </div>
            </>
          )}

          {activeTab === "auth" && (
            <>
              <div>
                <label className="form-label" style={{ display: "block", marginBottom: 6 }}>
                  Connection Mode
                </label>
                <div className="mode-grid">
                  {(["admin", "operator", "tenant"] as ConnectionMode[]).map((m) => (
                    <button
                      key={m}
                      className={`mode-card ${form.mode === m ? "selected" : ""}`}
                      onClick={() => patch("mode", m)}
                    >
                      <div className="mc-icon">
                        {m === "admin" ? "🛡" : m === "operator" ? "⚙" : "👤"}
                      </div>
                      <div className="mc-title">
                        {m.charAt(0).toUpperCase() + m.slice(1)}
                      </div>
                      <div className="mc-desc">
                        {m === "admin" ? "Full access via API key" : m === "operator" ? "Service-level access" : "Isolated tenant scope"}
                      </div>
                    </button>
                  ))}
                </div>
              </div>

              {form.mode === "admin" && (
                <div className="form-row">
                  <div className="form-field full">
                    <label className="form-label">Admin API Key</label>
                    <input
                      className="form-input"
                      type="password"
                      value={adminKey}
                      onChange={(e) => setAdminKey(e.target.value)}
                      placeholder="x-vng-admin-key value"
                      autoComplete="off"
                    />
                  </div>
                </div>
              )}
              {form.mode === "operator" && (
                <div className="form-row">
                  <div className="form-field full">
                    <label className="form-label">Operator ID</label>
                    <input
                      className="form-input"
                      value={form.operatorId ?? ""}
                      onChange={(e) => patch("operatorId", e.target.value || undefined)}
                      placeholder="op-xxxxxxxx"
                    />
                  </div>
                </div>
              )}
              {form.mode === "tenant" && (
                <div className="form-row">
                  <div className="form-field">
                    <label className="form-label">Tenant ID</label>
                    <input
                      className="form-input"
                      value={form.tenantId ?? ""}
                      onChange={(e) => patch("tenantId", e.target.value || undefined)}
                    />
                  </div>
                  <div className="form-field">
                    <label className="form-label">User ID</label>
                    <input
                      className="form-input"
                      value={form.userId ?? ""}
                      onChange={(e) => patch("userId", e.target.value || undefined)}
                    />
                  </div>
                </div>
              )}
            </>
          )}

          {activeTab === "ssl" && (
            <div style={{ color: "var(--text-3)", fontSize: 12, padding: "20px 0", textAlign: "center" }}>
              SSL / TLS configuration — coming in v0.2
            </div>
          )}

          {activeTab === "advanced" && (
            <div style={{ color: "var(--text-3)", fontSize: 12, padding: "20px 0", textAlign: "center" }}>
              Advanced settings (timeouts, pool size) — coming in v0.2
            </div>
          )}

          {error && (
            <div style={{ color: "var(--red)", fontSize: 11.5 }}>⚠ {error}</div>
          )}
        </div>

        {/* Footer */}
        <div className="conn-panel-footer">
          {testState !== "idle" && (
            <span className={`test-status ${testStatusClass}`}>
              {testStatusIcon} {testMsg}
            </span>
          )}
          <div style={{ flex: 1 }} />
          <button
            className="btn-wide secondary"
            style={{ width: 130 }}
            onClick={testConnection}
            disabled={testState === "testing"}
          >
            Test Connection
          </button>
          <button className="btn-wide primary" style={{ width: 130 }} onClick={save}>
            {existing ? "Save Changes" : "Save & Connect"}
          </button>
        </div>
      </div>
    </div>
  );
}
