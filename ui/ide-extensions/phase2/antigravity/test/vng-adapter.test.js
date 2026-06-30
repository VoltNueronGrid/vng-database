// D-5: Antigravity adapter smoke test. Runs with `node --test` — no IDE host,
// no live server required (HTTP is stubbed). Live IDE-runtime validation is
// tracked under E-5; an optional live check runs when VNG_IDE_LIVE=1.

'use strict';

const test = require('node:test');
const assert = require('node:assert');
const { VngAdapterClient, authHeaders, parseExecuteResponse } = require('../src/vng-adapter.js');

test('authHeaders include admin key + operator id + database', () => {
  const h = authHeaders({ adminKey: 'secret', database: 'sales' });
  assert.equal(h['x-vng-admin-key'], 'secret');
  assert.equal(h['x-vng-operator-id'], 'admin');
  assert.equal(h['x-vng-database'], 'sales');
});

test('parseExecuteResponse handles object columns + object rows', () => {
  const body = {
    status: 'ok',
    route_path: 'oltp',
    columns: [{ name: 'id', data_type: 'integer' }, { name: 'name', data_type: 'text' }],
    rows: [{ id: 1, name: 'alice' }, { id: 2, name: 'bob' }],
  };
  const r = parseExecuteResponse(body);
  assert.deepEqual(r.columns, ['id', 'name']);
  assert.equal(r.rows.length, 2);
  assert.equal(r.rows[0][0], '1');
  assert.equal(r.rows[0][1], 'alice');
});

test('parseExecuteResponse handles array rows', () => {
  const r = parseExecuteResponse({ columns: ['a', 'b'], rows: [['1', 'x']] });
  assert.equal(r.rows[0][1], 'x');
});

test('runQuery returns rows via stubbed fetch', async () => {
  const stub = async (url, opts) => {
    assert.ok(url.endsWith('/api/v1/sql/execute'));
    assert.equal(opts.headers['x-vng-admin-key'], 'k');
    return {
      status: 200,
      text: async () =>
        JSON.stringify({ status: 'ok', route_path: 'oltp', columns: ['id'], rows: [['42']] }),
    };
  };
  const client = new VngAdapterClient({ adminKey: 'k', fetchImpl: stub });
  const res = await client.runQuery('SELECT id FROM t');
  assert.equal(res.ok, true);
  assert.equal(res.rows[0][0], '42');
});

test('runQuery surfaces HTTP errors', async () => {
  const stub = async () => ({ status: 403, text: async () => '{"reason":"forbidden"}' });
  const client = new VngAdapterClient({ adminKey: 'k', fetchImpl: stub });
  const res = await client.runQuery('SELECT 1');
  assert.equal(res.ok, false);
  assert.match(res.error, /403/);
});

test('diagnostics reports reachability + roundtrip + auth checks', async () => {
  const stub = async (url) => {
    if (url.endsWith('/health')) return { status: 200, text: async () => 'ok' };
    return { status: 200, text: async () => JSON.stringify({ status: 'ok', route_path: 'oltp', columns: ['x'], rows: [['1']] }) };
  };
  const client = new VngAdapterClient({ adminKey: 'k', fetchImpl: stub });
  const checks = await client.diagnostics();
  const byName = Object.fromEntries(checks.map((c) => [c.name, c]));
  assert.equal(byName.server_reachable.ok, true);
  assert.equal(byName.query_roundtrip.ok, true);
  assert.equal(byName.auth_configured.ok, true);
});

test('diagnostics flags an unreachable server', async () => {
  const stub = async () => { throw new Error('ECONNREFUSED'); };
  const client = new VngAdapterClient({ adminKey: '', fetchImpl: stub });
  const checks = await client.diagnostics();
  const byName = Object.fromEntries(checks.map((c) => [c.name, c]));
  assert.equal(byName.server_reachable.ok, false);
  assert.equal(byName.auth_configured.ok, false);
});

// Optional live check — only when VNG_IDE_LIVE=1 against a running server.
test('live: query roundtrip against a real server', { skip: process.env.VNG_IDE_LIVE !== '1' }, async () => {
  const client = new VngAdapterClient({ adminKey: process.env.VNG_ADMIN_API_KEY || 'secret' });
  assert.equal(await client.health(), true);
  const table = `ag_demo_${Date.now()}`;
  await client.runQuery(`CREATE TABLE ${table} (id INT PRIMARY KEY, name TEXT)`);
  await client.runQuery(`INSERT INTO ${table} (id, name) VALUES (1, 'alice')`);
  const res = await client.runQuery(`SELECT id, name FROM ${table}`);
  assert.equal(res.ok, true);
  assert.ok(res.rows.length >= 1);
});
