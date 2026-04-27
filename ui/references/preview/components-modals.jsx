/* global React */
const { useState: useStateMd } = React;

/* ───────────────── Connection Panel ───────────────── */
function ConnectionPanel({ state, dispatch }) {
  const editing = state.editingConnId
    ? state.connections.find((c) => c.id === state.editingConnId)
    : null;
  const [form, setForm] = useStateMd(() => editing ?? {
    name: "", host: "", port: 7423, serverType: "voltnuerongrid", mode: "admin",
  });
  const [tab, setTab] = useStateMd("connection");
  const [test, setTest] = useStateMd(null);

  function field(key, val) { setForm((f) => ({ ...f, [key]: val })); }

  function runTest() {
    setTest("testing");
    setTimeout(() => setTest(form.host && form.host.includes("error") ? "fail" : "ok"), 700);
  }

  return (
    <div className="overlay" onClick={(e) => e.target === e.currentTarget && dispatch({ type: "closeConnPanel" })}>
      <div className="conn-panel">
        <div className="conn-panel-header">
          <div className="logo-icon" style={{ width: 28, height: 28, fontSize: 14 }}>{editing ? "✎" : "＋"}</div>
          <span className="conn-panel-title">{editing ? "Edit Connection" : "New Connection"}</span>
          <button className="conn-panel-close" onClick={() => dispatch({ type: "closeConnPanel" })}>✕</button>
        </div>
        <div className="conn-panel-tabs">
          {[["connection", "Connection"], ["advanced", "Advanced"], ["ssl", "SSL"]].map(([id, label]) => (
            <button key={id} className={window.cx("cp-tab", tab === id && "active")} onClick={() => setTab(id)}>{label}</button>
          ))}
        </div>
        <div className="conn-panel-body">
          {tab === "connection" && <>
            <div className="form-row">
              <div className="form-field full">
                <label className="form-label">Name</label>
                <input className="form-input" value={form.name} onChange={(e) => field("name", e.target.value)} placeholder="My Cluster" autoFocus />
              </div>
            </div>
            <div className="form-field">
              <label className="form-label">Server Type</label>
              <div className="mode-grid">
                {[
                  { id: "voltnuerongrid", icon: "⚡", t: "VNG", d: "Hybrid OLTP/OLAP" },
                  { id: "postgres",       icon: "🐘", t: "PostgreSQL", d: "Standard PG wire" },
                  { id: "duckdb",         icon: "🦆", t: "DuckDB", d: "Analytical replica" },
                ].map((o) => (
                  <button key={o.id} className={window.cx("mode-card", form.serverType === o.id && "selected")} onClick={() => field("serverType", o.id)}>
                    <div className="mc-icon">{o.icon}</div>
                    <div className="mc-title">{o.t}</div>
                    <div className="mc-desc">{o.d}</div>
                  </button>
                ))}
              </div>
            </div>
            <div className="form-row">
              <div className="form-field">
                <label className="form-label">Host</label>
                <input className="form-input" value={form.host} onChange={(e) => field("host", e.target.value)} placeholder="hostname or IP" />
              </div>
              <div className="form-field">
                <label className="form-label">Port</label>
                <input className="form-input" type="number" value={form.port} onChange={(e) => field("port", +e.target.value)} />
              </div>
            </div>
            <div className="form-field">
              <label className="form-label">Auth Mode</label>
              <div className="mode-grid">
                {[
                  { id: "admin",    t: "Admin",    d: "API key" },
                  { id: "operator", t: "Operator", d: "Operator JWT" },
                  { id: "readonly", t: "Read-only",d: "Service token" },
                ].map((o) => (
                  <button key={o.id} className={window.cx("mode-card", form.mode === o.id && "selected")} onClick={() => field("mode", o.id)}>
                    <div className="mc-title">{o.t}</div>
                    <div className="mc-desc">{o.d}</div>
                  </button>
                ))}
              </div>
            </div>
            <div className="form-row">
              <div className="form-field full">
                <label className="form-label">{form.mode === "admin" ? "Admin API Key" : "Token"}</label>
                <input className="form-input" type="password" placeholder="•••••••••••" />
              </div>
            </div>
          </>}
          {tab === "advanced" && (
            <div style={{ color: "var(--text-3)", fontSize: 12, lineHeight: 1.7 }}>
              Pool size, statement timeout, route hint preferences, connect timeout, retry policy.
              <br /><span style={{ opacity: .6 }}>(Demo — fields populated from sensible defaults.)</span>
            </div>
          )}
          {tab === "ssl" && (
            <div style={{ color: "var(--text-3)", fontSize: 12, lineHeight: 1.7 }}>
              SSL mode, CA cert, client cert + key. Verify-full enforced for admin connections.
            </div>
          )}
        </div>
        <div className="conn-panel-footer">
          <span className={window.cx("test-status", test)}>
            {test === "testing" && <>⟳ Testing…</>}
            {test === "ok" && <>✓ Connected · 12ms</>}
            {test === "fail" && <>✕ Could not connect</>}
            {!test && <>Click "Test" to verify</>}
          </span>
          <div style={{ flex: 1 }} />
          <button className="btn-wide secondary" style={{ width: 90 }} onClick={runTest}>Test</button>
          <button className="btn-wide secondary" style={{ width: 90 }} onClick={() => dispatch({ type: "closeConnPanel" })}>Cancel</button>
          <button className="btn-wide primary" style={{ width: 110 }} onClick={() => {
            dispatch({ type: "toast", msg: editing ? "Connection updated" : "Connection saved" });
            dispatch({ type: "closeConnPanel" });
          }}>{editing ? "Save" : "Connect"}</button>
        </div>
      </div>
    </div>
  );
}

/* ───────────────── Resource Modal ───────────────── */
const MODAL_TITLES = {
  "create-database": "Create Database",
  "drop-database":   "Drop Database",
  "create-schema":   "Create Schema",
  "drop-schema":     "Drop Schema",
  "create-table":    "Create Table",
  "drop-table":      "Drop Table",
  "truncate-table":  "Truncate Table",
  "rename-table":    "Rename",
  "edit-column":     "Edit Column",
  "drop-column":     "Drop Column",
  "create-user":     "Create User",
  "drop-user":       "Drop User",
  "create-role":     "Create Role",
  "grant-role":      "Grant Role",
  "view-ddl":        "DDL Preview",
};

function Field({ label, children, full }) {
  return (
    <div className={window.cx("form-field", full && "full")}>
      <label className="form-label">{label}</label>
      {children}
    </div>
  );
}

function ModalFooter({ onCancel, onSubmit, label = "Generate SQL", danger = false, disabled = false }) {
  return (
    <div className="conn-panel-footer" style={{ borderTop: "1px solid var(--border)", marginTop: 14 }}>
      <div style={{ flex: 1 }} />
      <button className="btn-wide secondary" style={{ width: 110 }} onClick={onCancel}>Cancel</button>
      <button
        className="btn-wide primary"
        style={{ width: 170, background: danger ? "var(--red)" : undefined, opacity: disabled ? 0.5 : 1 }}
        onClick={() => !disabled && onSubmit()}
      >{label}</button>
    </div>
  );
}

function CreateDatabaseForm({ dispatch, close }) {
  const [name, setName] = useStateMd("");
  const [enc, setEnc] = useStateMd("UTF8");
  const [route, setRoute] = useStateMd("hybrid");
  return <>
    <div className="form-row">
      <Field label="Database Name" full>
        <input className="form-input" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. analytics" autoFocus />
      </Field>
    </div>
    <div className="form-row">
      <Field label="Encoding">
        <select className="form-select" value={enc} onChange={(e) => setEnc(e.target.value)}>
          <option>UTF8</option><option>LATIN1</option><option>SQL_ASCII</option>
        </select>
      </Field>
      <Field label="Route Hint">
        <select className="form-select" value={route} onChange={(e) => setRoute(e.target.value)}>
          <option value="oltp">OLTP</option><option value="olap">OLAP</option><option value="hybrid">Hybrid</option>
        </select>
      </Field>
    </div>
    <ModalFooter onCancel={close} disabled={!name} onSubmit={() => {
      dispatch({ type: "openSqlTab", sql: `CREATE DATABASE ${name}\n  ENCODING '${enc}'\n  WITH (route = '${route}');`, name: `create_${name}.sql` });
      close();
    }} />
  </>;
}

function CreateSchemaForm({ ctx, dispatch, close }) {
  const [name, setName] = useStateMd("");
  return <>
    <div className="form-row">
      <Field label="Schema Name">
        <input className="form-input" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. reporting" autoFocus />
      </Field>
      <Field label="Owner (optional)">
        <input className="form-input" placeholder="user or role" />
      </Field>
    </div>
    <ModalFooter onCancel={close} disabled={!name} onSubmit={() => {
      dispatch({ type: "openSqlTab", sql: `CREATE SCHEMA ${ctx.target}.${name};`, name: `create_schema_${name}.sql` });
      close();
    }} />
  </>;
}

const TYPE_CHOICES = ["INT", "BIGINT", "SMALLINT", "DECIMAL(10,2)", "VARCHAR(255)", "TEXT", "BOOLEAN", "DATE", "TIMESTAMP", "JSON", "BLOB", "UUID"];

function CreateTableForm({ ctx, dispatch, close }) {
  const [name, setName] = useStateMd("");
  const [cols, setCols] = useStateMd([
    { name: "id", type: "BIGINT", pk: true, nullable: false, def: "" },
    { name: "created_at", type: "TIMESTAMP", pk: false, nullable: false, def: "CURRENT_TIMESTAMP" },
  ]);
  function update(i, p) { setCols((cs) => cs.map((c, idx) => idx === i ? { ...c, ...p } : c)); }
  function add() { setCols((cs) => [...cs, { name: "", type: "VARCHAR(255)", pk: false, nullable: true, def: "" }]); }
  function remove(i) { setCols((cs) => cs.filter((_, idx) => idx !== i)); }

  return <>
    <div className="form-row">
      <Field label="Table Name" full>
        <input className="form-input" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. orders" autoFocus />
      </Field>
    </div>
    <div className="detail-title" style={{ marginTop: 8 }}>Columns</div>
    <div style={{ background: "var(--bg-2)", border: "1px solid var(--border)", borderRadius: "var(--r-sm)", padding: 6, maxHeight: 280, overflowY: "auto" }}>
      <div style={{ display: "grid", gridTemplateColumns: "1.5fr 1.5fr 50px 50px 1.2fr 28px", gap: 6, padding: "2px 4px", fontSize: 10, color: "var(--text-3)", textTransform: "uppercase", letterSpacing: ".06em", fontWeight: 700 }}>
        <span>Name</span><span>Type</span><span>PK</span><span>Null</span><span>Default</span><span></span>
      </div>
      {cols.map((c, i) => (
        <div key={i} style={{ display: "grid", gridTemplateColumns: "1.5fr 1.5fr 50px 50px 1.2fr 28px", gap: 6, padding: "3px 4px", alignItems: "center" }}>
          <input className="form-input" style={{ height: 26, fontSize: 11.5 }} value={c.name} onChange={(e) => update(i, { name: e.target.value })} placeholder="column" />
          <select className="form-select" style={{ height: 26, fontSize: 11 }} value={c.type} onChange={(e) => update(i, { type: e.target.value })}>
            {TYPE_CHOICES.map((t) => <option key={t}>{t}</option>)}
          </select>
          <input type="checkbox" checked={c.pk} onChange={(e) => update(i, { pk: e.target.checked })} />
          <input type="checkbox" checked={c.nullable} onChange={(e) => update(i, { nullable: e.target.checked })} />
          <input className="form-input" style={{ height: 26, fontSize: 11.5 }} value={c.def} onChange={(e) => update(i, { def: e.target.value })} placeholder="—" />
          <button className="btn btn-sm" style={{ padding: "0 6px" }} onClick={() => remove(i)}>✕</button>
        </div>
      ))}
    </div>
    <button className="btn btn-sm" style={{ alignSelf: "flex-start", marginTop: 6 }} onClick={add}>＋ Add Column</button>
    <ModalFooter onCancel={close} disabled={!name} onSubmit={() => {
      const lines = cols.filter((c) => c.name).map((c) => {
        const parts = [`  ${c.name} ${c.type}`];
        if (!c.nullable) parts.push("NOT NULL");
        if (c.def) parts.push(`DEFAULT ${c.def}`);
        return parts.join(" ");
      });
      const pks = cols.filter((c) => c.pk).map((c) => c.name);
      if (pks.length) lines.push(`  PRIMARY KEY (${pks.join(", ")})`);
      dispatch({ type: "openSqlTab", sql: `CREATE TABLE ${ctx.target}.${name} (\n${lines.join(",\n")}\n);`, name: `create_${name}.sql` });
      close();
    }} />
  </>;
}

function DropForm({ ctx, what, dispatch, close }) {
  const [confirm, setConfirm] = useStateMd("");
  const [cascade, setCascade] = useStateMd(false);
  const last = ctx.target.split(".").pop();
  return <>
    <div style={{ background: "#ef444411", border: "1px solid #ef444433", borderRadius: "var(--r-sm)", padding: "10px 12px", color: "var(--red)", fontSize: 12.5, marginBottom: 10 }}>
      ⚠ This will permanently drop <strong>{ctx.target}</strong>. This action cannot be undone.
    </div>
    <Field label={`Type "${last}" to confirm`} full>
      <input className="form-input" value={confirm} onChange={(e) => setConfirm(e.target.value)} autoFocus />
    </Field>
    {what !== "USER" && (
      <label style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, marginTop: 8, color: "var(--text-2)" }}>
        <input type="checkbox" checked={cascade} onChange={(e) => setCascade(e.target.checked)} /> CASCADE — drop dependent objects
      </label>
    )}
    <ModalFooter onCancel={close} danger label={`Drop ${what}`} disabled={confirm !== last} onSubmit={() => {
      dispatch({ type: "openSqlTab", sql: `DROP ${what} IF EXISTS ${ctx.target}${cascade ? " CASCADE" : ""};`, name: `drop_${last}.sql` });
      close();
    }} />
  </>;
}

function TruncateForm({ ctx, dispatch, close }) {
  const [confirm, setConfirm] = useStateMd("");
  const last = ctx.target.split(".").pop();
  return <>
    <div style={{ background: "#eab30811", border: "1px solid #eab30833", borderRadius: "var(--r-sm)", padding: "10px 12px", color: "var(--yellow)", fontSize: 12.5, marginBottom: 10 }}>
      ⚠ Truncate removes ALL rows from <strong>{ctx.target}</strong>.
    </div>
    <Field label={`Type "${last}" to confirm`} full>
      <input className="form-input" value={confirm} onChange={(e) => setConfirm(e.target.value)} autoFocus />
    </Field>
    <ModalFooter onCancel={close} danger label="Truncate" disabled={confirm !== last} onSubmit={() => {
      dispatch({ type: "openSqlTab", sql: `TRUNCATE TABLE ${ctx.target} RESTART IDENTITY;`, name: `truncate_${last}.sql` });
      close();
    }} />
  </>;
}

function CreateUserForm({ dispatch, close }) {
  const [name, setName] = useStateMd("");
  const [pw, setPw] = useStateMd("");
  const [role, setRole] = useStateMd("readonly");
  return <>
    <div className="form-row">
      <Field label="Username">
        <input className="form-input" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. analyst" autoFocus />
      </Field>
      <Field label="Default Role">
        <select className="form-select" value={role} onChange={(e) => setRole(e.target.value)}>
          <option value="readonly">readonly</option><option value="readwrite">readwrite</option>
          <option value="dba">dba</option><option value="operator">operator</option>
        </select>
      </Field>
    </div>
    <Field label="Password" full>
      <input className="form-input" type="password" value={pw} onChange={(e) => setPw(e.target.value)} />
    </Field>
    <ModalFooter onCancel={close} disabled={!name || !pw} onSubmit={() => {
      dispatch({ type: "openSqlTab", sql: `CREATE USER ${name} WITH PASSWORD '${pw}';\nGRANT ${role} TO ${name};`, name: `create_user_${name}.sql` });
      close();
    }} />
  </>;
}

function GrantRoleForm({ ctx, dispatch, close }) {
  const [role, setRole] = useStateMd("readonly");
  return <>
    <Field label="Role" full>
      <select className="form-select" value={role} onChange={(e) => setRole(e.target.value)}>
        <option value="readonly">readonly</option><option value="readwrite">readwrite</option>
        <option value="dba">dba</option><option value="operator">operator</option>
      </select>
    </Field>
    <ModalFooter onCancel={close} onSubmit={() => {
      dispatch({ type: "openSqlTab", sql: `GRANT ${role} TO ${ctx.target};`, name: `grant_${ctx.target}.sql` });
      close();
    }} />
  </>;
}

function ViewDdlForm({ ctx, dispatch, close }) {
  const ddl = (() => {
    const t = ctx.payload && ctx.payload.table;
    if (t) {
      const lines = t.columns.map((c) => `  ${c.name} ${c.data_type}${c.nullable ? "" : " NOT NULL"}${c.primary_key ? " PRIMARY KEY" : ""}`);
      return `CREATE TABLE ${ctx.target} (\n${lines.join(",\n")}\n);`;
    }
    return `-- DDL for ${ctx.target}\n-- (server returns synthesized DDL — preview only)`;
  })();
  return <>
    <textarea className="form-input mono" value={ddl} readOnly rows={14}
      style={{ height: "auto", padding: 10, fontSize: 12, lineHeight: 1.5, resize: "vertical" }} />
    <ModalFooter onCancel={close} label="Open in Editor" onSubmit={() => {
      dispatch({ type: "openSqlTab", sql: ddl, name: `ddl_${ctx.target}.sql` });
      close();
    }} />
  </>;
}

function ResourceModal({ ctx, dispatch }) {
  const close = () => dispatch({ type: "closeModal" });
  const isDanger = ctx.kind.startsWith("drop-") || ctx.kind === "truncate-table";
  return (
    <div className="overlay" onClick={(e) => e.target === e.currentTarget && close()}>
      <div className="conn-panel" style={{ width: ctx.kind === "create-table" ? 720 : 520 }}>
        <div className="conn-panel-header">
          <div className="logo-icon" style={{
            width: 28, height: 28, fontSize: 14,
            background: isDanger
              ? "linear-gradient(135deg,#ef4444,#9333ea)"
              : "linear-gradient(135deg,var(--brand-cyan),var(--brand-purple))",
          }}>{isDanger ? "!" : "+"}</div>
          <span className="conn-panel-title">{MODAL_TITLES[ctx.kind] || "Resource"}</span>
          {ctx.target && <span className="mono" style={{ marginLeft: 8, fontSize: 11, color: "var(--text-3)" }}>{ctx.target}</span>}
          <button className="conn-panel-close" onClick={close}>✕</button>
        </div>
        <div className="conn-panel-body">
          {ctx.kind === "create-database" && <CreateDatabaseForm dispatch={dispatch} close={close} />}
          {ctx.kind === "create-schema"   && <CreateSchemaForm ctx={ctx} dispatch={dispatch} close={close} />}
          {ctx.kind === "create-table"    && <CreateTableForm ctx={ctx} dispatch={dispatch} close={close} />}
          {ctx.kind === "drop-database"   && <DropForm ctx={ctx} what="DATABASE" dispatch={dispatch} close={close} />}
          {ctx.kind === "drop-schema"     && <DropForm ctx={ctx} what="SCHEMA"   dispatch={dispatch} close={close} />}
          {ctx.kind === "drop-table"      && <DropForm ctx={ctx} what="TABLE"    dispatch={dispatch} close={close} />}
          {ctx.kind === "drop-user"       && <DropForm ctx={ctx} what="USER"     dispatch={dispatch} close={close} />}
          {ctx.kind === "drop-column"     && <DropForm ctx={ctx} what="COLUMN"   dispatch={dispatch} close={close} />}
          {ctx.kind === "drop-schema"     && <DropForm ctx={ctx} what="SCHEMA"   dispatch={dispatch} close={close} />}
          {ctx.kind === "truncate-table"  && <TruncateForm ctx={ctx} dispatch={dispatch} close={close} />}
          {ctx.kind === "create-user"     && <CreateUserForm dispatch={dispatch} close={close} />}
          {ctx.kind === "grant-role"      && <GrantRoleForm ctx={ctx} dispatch={dispatch} close={close} />}
          {ctx.kind === "view-ddl"        && <ViewDdlForm ctx={ctx} dispatch={dispatch} close={close} />}
          {(ctx.kind === "rename-table" || ctx.kind === "edit-column" || ctx.kind === "create-role") && (
            <>
              <Field label="Name" full>
                <input className="form-input" placeholder="…" autoFocus />
              </Field>
              <ModalFooter onCancel={close} onSubmit={() => {
                dispatch({ type: "toast", msg: `(Demo) ${MODAL_TITLES[ctx.kind]} would generate SQL` });
                close();
              }} />
            </>
          )}
        </div>
      </div>
    </div>
  );
}

window.ConnectionPanel = ConnectionPanel;
window.ResourceModal = ResourceModal;
