/* Sample data for the Studio preview */

window.SAMPLE_CONNECTIONS = [
  { id: "c1", name: "Production VNG",  host: "vng-prod.internal", port: 7423, mode: "admin",    serverType: "voltnuerongrid", health: "ok"   },
  { id: "c2", name: "Staging VNG",     host: "vng-stage.dev",     port: 7423, mode: "operator", serverType: "voltnuerongrid", health: "ok"   },
  { id: "c3", name: "Postgres Replica",host: "pg.read.local",     port: 5432, mode: "readonly", serverType: "postgres",       health: "error"},
  { id: "c4", name: "Local Sandbox",   host: "127.0.0.1",         port: 7423, mode: "admin",    serverType: "voltnuerongrid", health: "none" },
];

window.SAMPLE_SCHEMA = {
  databases: [
    {
      name: "analytics",
      schemas: [
        {
          name: "public",
          tables: [
            { name: "events",        row_count: 4_812_001, schema: "public", columns: [
              { name: "id", data_type: "BIGINT", primary_key: true, nullable: false },
              { name: "user_id", data_type: "BIGINT", primary_key: false, nullable: false },
              { name: "kind", data_type: "VARCHAR(64)", primary_key: false, nullable: false },
              { name: "payload", data_type: "JSON", primary_key: false, nullable: true },
              { name: "created_at", data_type: "TIMESTAMP", primary_key: false, nullable: false },
            ]},
            { name: "sessions",      row_count: 821_403, schema: "public", columns: [
              { name: "id", data_type: "UUID", primary_key: true, nullable: false },
              { name: "user_id", data_type: "BIGINT", primary_key: false, nullable: false },
              { name: "started_at", data_type: "TIMESTAMP", primary_key: false, nullable: false },
              { name: "ended_at", data_type: "TIMESTAMP", primary_key: false, nullable: true },
              { name: "is_active", data_type: "BOOLEAN", primary_key: false, nullable: false },
            ]},
            { name: "page_views",    row_count: 12_004_011, schema: "public", columns: [
              { name: "id", data_type: "BIGINT", primary_key: true, nullable: false },
              { name: "session_id", data_type: "UUID", primary_key: false, nullable: false },
              { name: "url", data_type: "TEXT", primary_key: false, nullable: false },
              { name: "duration_ms", data_type: "INT", primary_key: false, nullable: true },
            ]},
          ]
        },
        {
          name: "reporting",
          tables: [
            { name: "daily_active",  row_count: 1024, schema: "reporting", columns: [
              { name: "day", data_type: "DATE", primary_key: true, nullable: false },
              { name: "count", data_type: "INT", primary_key: false, nullable: false },
              { name: "delta", data_type: "DECIMAL(10,2)", primary_key: false, nullable: true },
            ]},
            { name: "funnel",        row_count: 86, schema: "reporting", columns: [
              { name: "id", data_type: "INT", primary_key: true, nullable: false },
              { name: "step", data_type: "VARCHAR(64)", primary_key: false, nullable: false },
              { name: "rate", data_type: "DECIMAL(5,4)", primary_key: false, nullable: true },
            ]},
          ]
        },
      ]
    },
    {
      name: "billing",
      schemas: [
        {
          name: "public",
          tables: [
            { name: "customers",     row_count: 32_811, schema: "public", columns: [
              { name: "id", data_type: "BIGINT", primary_key: true, nullable: false },
              { name: "email", data_type: "VARCHAR(255)", primary_key: false, nullable: false },
              { name: "plan", data_type: "VARCHAR(32)", primary_key: false, nullable: false },
              { name: "created_at", data_type: "TIMESTAMP", primary_key: false, nullable: false },
            ]},
            { name: "invoices",      row_count: 218_403, schema: "public", columns: [
              { name: "id", data_type: "BIGINT", primary_key: true, nullable: false },
              { name: "customer_id", data_type: "BIGINT", primary_key: false, nullable: false },
              { name: "amount_cents", data_type: "INT", primary_key: false, nullable: false },
              { name: "paid", data_type: "BOOLEAN", primary_key: false, nullable: false },
              { name: "due_at", data_type: "DATE", primary_key: false, nullable: false },
            ]},
          ]
        }
      ]
    }
  ]
};

window.SAMPLE_USERS = [
  { id: "u1", username: "admin",    role: "dba",        active: true },
  { id: "u2", username: "analyst",  role: "readonly",   active: true },
  { id: "u3", username: "etl_bot",  role: "readwrite",  active: true },
  { id: "u4", username: "ops",      role: "operator",   active: true },
  { id: "u5", username: "former",   role: "readonly",   active: false },
];

window.SAMPLE_RESULT = {
  routePath: "olap",
  elapsedMs: 184,
  rowCount: 25,
  columns: [
    { name: "id", type: "BIGINT" },
    { name: "user_id", type: "BIGINT" },
    { name: "kind", type: "VARCHAR" },
    { name: "payload", type: "JSON" },
    { name: "created_at", type: "TIMESTAMP" },
  ],
  rows: Array.from({ length: 25 }, (_, i) => ({
    id: 1000 + i,
    user_id: 200 + (i % 7),
    kind: ["page_view", "click", "purchase", "signup"][i % 4],
    payload: i % 3 === 0 ? null : `{"v":${i}}`,
    created_at: `2025-04-${String((i % 26) + 1).padStart(2, "0")}T12:${String(i % 60).padStart(2, "0")}:00Z`,
  })),
};
