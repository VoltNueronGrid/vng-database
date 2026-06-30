// D-5: VoltNueronGrid adapter client for the Antigravity IDE.
//
// Dependency-free Node module (uses the built-in `fetch`/`http`) exposing a
// query runner and a diagnostics helper. The IDE-host wiring (command palette,
// secret storage via the host API) is thin glue over this module; this core is
// unit-tested with `node --test`.

'use strict';

/**
 * Build the RBAC auth headers for a request. An admin key plus operator id
 * (default `admin`) are sent so the server's SQL-runtime principal resolves.
 * @param {{adminKey?: string, operatorId?: string, database?: string}} cfg
 * @returns {Record<string,string>}
 */
function authHeaders(cfg) {
  const h = { 'content-type': 'application/json' };
  if (cfg.adminKey) {
    h['x-vng-admin-key'] = cfg.adminKey;
    h['x-vng-operator-id'] = cfg.operatorId || 'admin';
  }
  if (cfg.database) {
    h['x-vng-database'] = cfg.database;
  }
  return h;
}

/**
 * Normalise a `/api/v1/sql/execute` response body into `{ columns, rows }`.
 * Handles object columns (`{name,..}`) or bare strings, and object or array rows.
 * @param {any} body parsed JSON response
 * @returns {{ status: string, routePath: string, columns: string[], rows: string[][] }}
 */
function parseExecuteResponse(body) {
  if (!body || typeof body !== 'object') {
    return { status: 'error', routePath: '', columns: [], rows: [] };
  }
  const columns = [];
  if (Array.isArray(body.columns)) {
    for (const c of body.columns) {
      if (c && typeof c === 'object' && 'name' in c) {
        columns.push(String(c.name));
      } else {
        columns.push(scalar(c));
      }
    }
  }
  const rows = [];
  if (Array.isArray(body.rows)) {
    for (const row of body.rows) {
      if (Array.isArray(row)) {
        if (columns.length === 0) {
          for (let i = 0; i < row.length; i++) columns.push(`column${i}`);
        }
        rows.push(row.map(scalar));
      } else if (row && typeof row === 'object') {
        if (columns.length === 0) {
          columns.push(...Object.keys(row));
        }
        rows.push(columns.map((c) => scalar(row[c])));
      }
    }
  }
  return {
    status: String(body.status || 'ok'),
    routePath: String(body.route_path || ''),
    columns,
    rows,
  };
}

function scalar(v) {
  if (v === null || v === undefined) return null;
  if (typeof v === 'number' && Number.isInteger(v)) return String(v);
  return String(v);
}

/**
 * Adapter client. `fetchImpl` is injected so tests can stub HTTP without a
 * server; in the IDE it defaults to the global `fetch`.
 */
class VngAdapterClient {
  /**
   * @param {{host?: string, port?: number, adminKey?: string, operatorId?: string,
   *          database?: string, fetchImpl?: Function}} cfg
   */
  constructor(cfg = {}) {
    this.host = cfg.host || '127.0.0.1';
    this.port = cfg.port || 8080;
    this.adminKey = cfg.adminKey || '';
    this.operatorId = cfg.operatorId || 'admin';
    this.database = cfg.database || '';
    this._fetch = cfg.fetchImpl || globalThis.fetch;
  }

  baseUrl() {
    return `http://${this.host}:${this.port}`;
  }

  /** GET /health → boolean. */
  async health() {
    try {
      const res = await this._fetch(`${this.baseUrl()}/health`, {
        method: 'GET',
        headers: authHeaders(this),
      });
      return res.status === 200;
    } catch {
      return false;
    }
  }

  /** Run a SQL batch and return `{ ok, columns, rows, error }`. */
  async runQuery(sql) {
    try {
      const res = await this._fetch(`${this.baseUrl()}/api/v1/sql/execute`, {
        method: 'POST',
        headers: authHeaders(this),
        body: JSON.stringify({ sql_batch: sql }),
      });
      const text = await res.text();
      if (res.status !== 200) {
        return { ok: false, error: `HTTP ${res.status}: ${text}`, columns: [], rows: [] };
      }
      const parsed = parseExecuteResponse(JSON.parse(text));
      return { ok: true, ...parsed };
    } catch (e) {
      return { ok: false, error: `transport error: ${e.message}`, columns: [], rows: [] };
    }
  }

  /**
   * Connection diagnostics for the IDE status panel: validates reachability and
   * a trivial round-trip query, returning a list of named check results.
   * @returns {Promise<Array<{name: string, ok: boolean, detail: string}>>}
   */
  async diagnostics() {
    const checks = [];
    const reachable = await this.health();
    checks.push({
      name: 'server_reachable',
      ok: reachable,
      detail: reachable ? `connected to ${this.baseUrl()}` : `cannot reach ${this.baseUrl()}`,
    });
    if (reachable) {
      const probe = await this.runQuery('SELECT 1');
      checks.push({
        name: 'query_roundtrip',
        ok: probe.ok,
        detail: probe.ok ? `route ${probe.routePath}` : probe.error,
      });
    }
    checks.push({
      name: 'auth_configured',
      ok: Boolean(this.adminKey),
      detail: this.adminKey ? 'admin key present' : 'no admin key configured',
    });
    return checks;
  }
}

module.exports = { VngAdapterClient, authHeaders, parseExecuteResponse };
