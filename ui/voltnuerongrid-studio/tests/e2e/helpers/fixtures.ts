import { test as base, Page, Route } from "@playwright/test";

// ─── Mock API Responses ──────────────────────────────────────────────────────

export const MOCK_HEALTH = { status: "ok", version: "0.1.0", uptime_ms: 12345 };

export const MOCK_TOPOLOGY = {
  leader_node_id: "node-1",
  total_nodes: 3,
  active_nodes: 3,
  passive_nodes: 0,
  dead_nodes: 0,
  active_sessions: 12,
  passive_sessions: 3,
  live_transactions: 4,
  total_transactions: 1200,
  live_locks: 2,
  nodes: [
    {
      node_id: "node-1",
      role: "primary",
      status: "active",
      total_cpu_cores: 8,
      total_ram_mb: 16384,
      used_cpu_pct: 25.5,
      used_ram_mb: 4096,
      active_sessions: 6,
      live_transactions: 2,
      total_transactions: 600,
      live_locks: 1,
      draining: false,
    },
  ],
};

export const MOCK_AUDIT_EVENTS = {
  status: "ok",
  total_events: 2,
  events: [
    {
      event_id: 1,
      occurred_epoch_ms: Date.now() - 10_000,
      actor: "admin",
      action: "query.execute",
      kind: "sql",
      outcome: "success",
      details_json: "{}",
    },
    {
      event_id: 2,
      occurred_epoch_ms: Date.now() - 60_000,
      actor: "operator1",
      action: "connection.created",
      kind: "auth",
      outcome: "success",
      details_json: "{}",
    },
  ],
};

export const MOCK_QUERY_RESULT = {
  status: "ok",
  route_path: "oltp",
  reason: "point lookup",
  rejected_statement_count: 0,
  transaction: {
    status: "ok",
    transaction_id: "txn-1",
    statements_executed: 1,
    requires_transaction: false,
    touches_catalog: false,
    rejected_statement_count: 0,
    elapsed_ms: 42,
  },
  columns: [
    { name: "id", data_type: "integer" },
    { name: "name", data_type: "varchar" },
    { name: "value", data_type: "integer" },
  ],
  rows: [
    { id: 1, name: "alpha", value: 100 },
    { id: 2, name: "beta", value: 200 },
    { id: 3, name: "gamma", value: 300 },
  ],
};

export const MOCK_SCHEMA = {
  databases: [
    {
      name: "default",
      schemas: [
        {
          name: "public",
          database: "default",
          tables: [
            {
              schema: "public",
              name: "users",
              columns: [
                { name: "id", data_type: "integer", nullable: false, primary_key: true },
                { name: "email", data_type: "varchar", nullable: false, primary_key: false },
              ],
              row_count: 1000,
            },
            {
              schema: "public",
              name: "orders",
              columns: [
                { name: "id", data_type: "integer", nullable: false, primary_key: true },
                { name: "user_id", data_type: "integer", nullable: false, primary_key: false },
                { name: "amount", data_type: "decimal", nullable: true, primary_key: false },
              ],
              row_count: 5000,
            },
          ],
        },
      ],
    },
  ],
};

// ─── Route Mocking ────────────────────────────────────────────────────────────

export async function mockApiRoutes(page: Page) {
  await page.route("**/health", (route: Route) => {
    route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(MOCK_HEALTH) });
  });

  await page.route("**/api/v1/admin/cluster/topology", (route: Route) => {
    route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(MOCK_TOPOLOGY) });
  });

  await page.route("**/api/v1/audit/events**", (route: Route) => {
    route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(MOCK_AUDIT_EVENTS) });
  });

  await page.route("**/api/v1/sql/execute", (route: Route) => {
    route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(MOCK_QUERY_RESULT) });
  });

  await page.route("**/api/v1/admin/schema/tree", (route: Route) => {
    route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(MOCK_SCHEMA) });
  });
}

// ─── LocalStorage Helpers ─────────────────────────────────────────────────────

/** The Zustand persist key used by useConnectionStore */
const CONN_STORE_KEY = "vng-studio-connections";

export async function seedConnection(
  page: Page,
  overrides: Record<string, unknown> = {}
) {
  const conn = {
    id: "conn-test-1",
    name: "Test VNG Server",
    serverType: "voltnuerongrid",
    runtimeTarget: "local",
    baseUrl: "http://127.0.0.1:8080",
    host: "127.0.0.1",
    port: 8080,
    mode: "admin",
    sslEnabled: false,
    createdAt: Date.now() - 100_000,
    lastUsed: Date.now() - 5_000,
    ...overrides,
  };

  await page.evaluate(
    ({ key, c }) => {
      const state = { state: { connections: [c], activeId: c.id }, version: 0 };
      localStorage.setItem(key, JSON.stringify(state));
    },
    { key: CONN_STORE_KEY, c: conn }
  );
}

export async function clearConnections(page: Page) {
  await page.evaluate((key) => localStorage.removeItem(key), CONN_STORE_KEY);
}

// ─── Custom Fixtures ──────────────────────────────────────────────────────────

type AppFixtures = {
  /** Page with all API routes mocked */
  mockedPage: Page;
  /** Page with mocked routes AND a pre-seeded connection, reloaded to welcome */
  connectedPage: Page;
};

export const test = base.extend<AppFixtures>({
  mockedPage: async ({ page }, use) => {
    await mockApiRoutes(page);
    await page.goto("/");
    await use(page);
  },

  connectedPage: async ({ page }, use) => {
    await mockApiRoutes(page);
    await page.goto("/");
    await seedConnection(page);
    await page.reload();
    // Navigate to main by clicking the first recent connection, or
    // fall back to direct store navigation via New Query card if list is empty.
    const recentItem = page.locator(".recent-item").first();
    const hasRecent = await recentItem.isVisible({ timeout: 3000 }).catch(() => false);
    if (hasRecent) {
      await recentItem.click();
    } else {
      await page.locator(".welcome-card").filter({ hasText: "New Query" }).click();
    }
    await use(page);
  },
});

export { expect } from "@playwright/test";
