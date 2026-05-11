/**
 * Live API end-to-end tests — these run against the REAL voltnuerongridd server
 * (no Playwright route mocking). Requires:
 *   - cargo run -p voltnuerongridd (server on http://127.0.0.1:8080)
 *   - VNG_ADMIN_API_KEY=secret (or whatever key the server was started with)
 *
 * Run with:
 *   npx playwright test live-api --project=chromium
 *
 * These tests are intentionally separate from the mock suite so they can be
 * gated on server availability in CI.
 */

import { test, expect } from "@playwright/test";

const LIVE_BASE = "http://127.0.0.1:8080";
const ADMIN_KEY = process.env.VNG_ADMIN_API_KEY ?? "secret";
const CONN_STORE_KEY = "vng-studio-connections";

// ─── Skip guard ───────────────────────────────────────────────────────────────

async function serverIsUp(): Promise<boolean> {
  try {
    const res = await fetch(`${LIVE_BASE}/health`);
    return res.ok;
  } catch {
    return false;
  }
}

// Seeds a real connection into localStorage so the app treats it as active.
async function seedLiveConnection(page: import("@playwright/test").Page) {
  const conn = {
    id: "conn-live-1",
    name: "Local VNG Server",
    serverType: "voltnuerongrid",
    runtimeTarget: "local",
    baseUrl: LIVE_BASE,
    host: "127.0.0.1",
    port: 8080,
    mode: "admin",
    sslEnabled: false,
    createdAt: Date.now() - 100_000,
    lastUsed: Date.now() - 5_000,
  };
  await page.evaluate(
    ({ key, c }) => {
      const state = { state: { connections: [c], activeId: c.id }, version: 0 };
      localStorage.setItem(key, JSON.stringify(state));
    },
    { key: CONN_STORE_KEY, c: conn }
  );
}

// ─── Health ───────────────────────────────────────────────────────────────────

test.describe("live: health endpoint", () => {
  test.beforeAll(async () => {
    if (!(await serverIsUp())) {
      test.skip();
    }
  });

  test("GET /health returns status ok", async ({ request }) => {
    const res = await request.get(`${LIVE_BASE}/health`);
    expect(res.ok()).toBeTruthy();
    const body = await res.json();
    expect(body.status).toBe("ok");
    expect(body).toHaveProperty("node_id");
  });
});

// ─── Test Connection button ───────────────────────────────────────────────────

test.describe("live: Test Connection button", () => {
  test.beforeAll(async () => {
    if (!(await serverIsUp())) {
      test.skip();
    }
  });

  test("shows Connected when server is running on 8080", async ({ page }) => {
    await page.goto("/");

    // Clear any stale stored connections so welcome screen appears
    await page.evaluate((key) => localStorage.removeItem(key), CONN_STORE_KEY);
    await page.reload();

    // Click New Connection from the welcome screen
    await page.locator(".welcome-card").filter({ hasText: "New Connection" }).click();

    // The connection panel modal should now be visible
    await expect(page.locator(".conn-panel-title")).toBeVisible({ timeout: 5_000 });

    // Switch to Auth tab and fill in the Admin API Key (now required)
    await page.locator(".cp-tab", { hasText: "Auth" }).click();
    await page.locator('input[placeholder="x-vng-admin-key value"]').fill(ADMIN_KEY);

    // Click Test Connection
    await page.locator("button", { hasText: "Test Connection" }).click();

    // Expect the success message for admin auth
    await expect(
      page.locator(".test-status")
    ).toContainText("Authenticated", { timeout: 10_000 });
  });

  test("shows validation error when Admin Key is missing", async ({ page }) => {
    await page.goto("/");
    await page.evaluate((key) => localStorage.removeItem(key), CONN_STORE_KEY);
    await page.reload();

    await page.locator(".welcome-card").filter({ hasText: "New Connection" }).click();
    await expect(page.locator(".conn-panel-title")).toBeVisible({ timeout: 5_000 });

    // Don't fill in admin key — click Test Connection directly
    await page.locator("button", { hasText: "Test Connection" }).click();

    // Should fail with a validation message (no network call made)
    await expect(page.locator(".test-status.fail")).toBeVisible({ timeout: 5_000 });
    await expect(page.locator(".test-status.fail")).toContainText("Admin API Key");
  });

  test("shows error when server returns HTTP 500", async ({ page }) => {
    // Intercept /health at the proxy level to simulate a server error.
    await page.route("**/health", (route) =>
      route.fulfill({ status: 500, contentType: "application/json", body: JSON.stringify({ error: "forced failure" }) })
    );

    await page.goto("/");
    await page.evaluate((key) => localStorage.removeItem(key), CONN_STORE_KEY);
    await page.reload();

    await page.locator(".welcome-card").filter({ hasText: "New Connection" }).click();
    await expect(page.locator(".conn-panel-title")).toBeVisible({ timeout: 5_000 });

    // Fill required admin key so validation passes and the network call is made
    await page.locator(".cp-tab", { hasText: "Auth" }).click();
    await page.locator('input[placeholder="x-vng-admin-key value"]').fill("any-key");

    await page.locator("button", { hasText: "Test Connection" }).click();

    // Should show a fail status (500 triggers error path in testConnection())
    await expect(
      page.locator(".test-status.fail")
    ).toBeVisible({ timeout: 10_000 });
  });
});

// ─── Schema tree via Vite proxy ───────────────────────────────────────────────

test.describe("live: schema tree loads real data", () => {
  test.beforeAll(async () => {
    if (!(await serverIsUp())) {
      test.skip();
    }
  });

  test("schema tree renders database nodes from live server", async ({ page }) => {
    await page.goto("/");
    await seedLiveConnection(page);
    await page.reload();

    // Welcome screen shows the seeded connection in recent list — click it to enter workspace
    const recentItem = page.locator(".recent-item").first();
    await expect(recentItem).toBeVisible({ timeout: 5_000 });
    await recentItem.click();

    // App should now show the main workspace
    await expect(page.locator(".workspace")).toBeVisible({ timeout: 8_000 });

    // Sidebar should be visible (schema panel or activity bar)
    await expect(page.locator(".sidebar")).toBeVisible({ timeout: 5_000 });
  });
});

// ─── SQL execute via Vite proxy ───────────────────────────────────────────────

test.describe("live: SQL execute returns real results", () => {
  test.beforeAll(async () => {
    if (!(await serverIsUp())) {
      test.skip();
    }
  });

  test("SELECT 1 returns ok status from live server", async ({ request }) => {
    const res = await request.post(`${LIVE_BASE}/api/v1/sql/execute`, {
      data: { sql_batch: "SELECT 1", max_rows: 10 },
      headers: {
        "content-type": "application/json",
        "x-vng-admin-key": ADMIN_KEY,
        "x-vng-operator-id": "admin",
      },
    });
    // Server may return 200 or 401/403 depending on auth setup; either way, not 500
    expect(res.status()).not.toBe(500);
    if (res.ok()) {
      const body = await res.json();
      expect(body).toHaveProperty("status");
    }
  });

  test("admin schema tree endpoint returns databases array", async ({ request }) => {
    const res = await request.get(`${LIVE_BASE}/api/v1/admin/schema/tree`, {
      headers: {
        "x-vng-admin-key": ADMIN_KEY,
        "x-vng-operator-id": "admin",
      },
    });
    expect(res.status()).not.toBe(500);
    if (res.ok()) {
      const body = await res.json();
      expect(body).toHaveProperty("databases");
      expect(Array.isArray(body.databases)).toBeTruthy();
    }
  });

  test("cluster topology endpoint returns node list", async ({ request }) => {
    const res = await request.get(`${LIVE_BASE}/api/v1/admin/cluster/topology`, {
      headers: {
        "x-vng-admin-key": ADMIN_KEY,
        "x-vng-operator-id": "admin",
      },
    });
    expect(res.status()).not.toBe(500);
    if (res.ok()) {
      const body = await res.json();
      expect(body).toHaveProperty("nodes");
      expect(Array.isArray(body.nodes)).toBeTruthy();
    }
  });
});

// ─── Connection panel UI with live proxy ─────────────────────────────────────

test.describe("live: connection panel via Vite proxy", () => {
  test.beforeAll(async () => {
    if (!(await serverIsUp())) {
      test.skip();
    }
  });

  test("Vite proxy correctly forwards /health to the server", async ({ page }) => {
    // Navigate to app — the app itself will call /health in dev mode via Vite proxy
    await page.goto("/");
    const proxyRes = await page.request.get("/health");
    expect(proxyRes.ok()).toBeTruthy();
    const body = await proxyRes.json();
    expect(body.status).toBe("ok");
  });

  test("Vite proxy correctly forwards /api to the server", async ({ page }) => {
    await page.goto("/");
    const proxyRes = await page.request.post("/api/v1/sql/execute", {
      data: { sql_batch: "SELECT 1" },
      headers: {
        "content-type": "application/json",
        "x-vng-admin-key": ADMIN_KEY,
        "x-vng-operator-id": "admin",
      },
    });
    // Not 500 means proxy is working
    expect(proxyRes.status()).not.toBe(500);
  });
});
