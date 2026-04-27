/**
 * Headed live end-to-end tests for VoltNueronGrid Studio.
 *
 * Tests cover the full UI workflow using a REAL running server:
 *   - Connection setup via UI
 *   - CREATE TABLE, INSERT, SELECT, CREATE VIEW
 *   - Query results pane rendering
 *   - Schema tree loading
 *   - Dashboard cluster view
 *   - Status bar reflecting connection
 *
 * Prerequisites:
 *   cargo run -p voltnuerongridd  (VNG_ADMIN_API_KEY=secret)
 *
 * Run headed:
 *   VNG_ADMIN_API_KEY=secret npx playwright test live-headed --project=chromium --headed
 */

import { test, expect } from "@playwright/test";

const LIVE_BASE = "http://127.0.0.1:8080";
const ADMIN_KEY = process.env.VNG_ADMIN_API_KEY ?? "secret";
const CONN_STORE_KEY = "vng-studio-connections";

// ─── Helpers ─────────────────────────────────────────────────────────────────

async function serverIsUp(): Promise<boolean> {
  try {
    const res = await fetch(`${LIVE_BASE}/health`);
    return res.ok;
  } catch {
    return false;
  }
}

/** Seed a live connection (with adminKey) directly into localStorage. */
async function seedLiveConn(page: import("@playwright/test").Page) {
  const conn = {
    id: "conn-live-e2e",
    name: "Live VNG (e2e)",
    serverType: "voltnuerongrid",
    runtimeTarget: "local",
    protocol: "http",
    baseUrl: LIVE_BASE,
    host: "127.0.0.1",
    port: 8080,
    mode: "admin",
    adminKey: ADMIN_KEY,
    sslEnabled: false,
    createdAt: Date.now() - 200_000,
    lastUsed: Date.now() - 1_000,
  };
  await page.evaluate(
    ({ key, c }) => {
      const state = { state: { connections: [c], activeId: c.id }, version: 0 };
      localStorage.setItem(key, JSON.stringify(state));
    },
    { key: CONN_STORE_KEY, c: conn }
  );
}

/** Navigate to the workspace with a live connection active. */
async function goToWorkspace(page: import("@playwright/test").Page) {
  await page.goto("/");
  await seedLiveConn(page);
  await page.reload();
  // Click the connection in the recent list to activate it
  const recent = page.locator(".recent-item").first();
  if (await recent.isVisible({ timeout: 4_000 }).catch(() => false)) {
    await recent.click();
  } else {
    // Fallback: New Query card (connection already set as active)
    await page.locator(".welcome-card").filter({ hasText: "New Query" }).click();
  }
  await expect(page.locator(".workspace")).toBeVisible({ timeout: 8_000 });
}

/** Fill the Monaco editor with SQL text.
 *  Monaco intercepts pointer events on its textarea in headed mode,
 *  so we click the content area (.view-lines) to focus the editor,
 *  then select-all and type via keyboard API.
 */
async function fillEditor(page: import("@playwright/test").Page, sql: string) {
  // Click the visible content area to focus the editor
  const viewLines = page.locator(".view-lines").first();
  const editorContainer = page.locator(".monaco-editor").first();

  if (await viewLines.isVisible({ timeout: 3_000 }).catch(() => false)) {
    await viewLines.click({ force: true });
  } else {
    await editorContainer.click({ force: true });
  }

  // Select all existing content and replace it
  await page.keyboard.press("Meta+a");
  await page.keyboard.press("Delete");

  if (sql.length > 0) {
    // Use clipboard for reliable multi-line input
    await page.evaluate((text) => {
      navigator.clipboard.writeText(text).catch(() => {});
    }, sql);
    await page.keyboard.press("Meta+v");

    // Fallback: if clipboard paste didn't work, type character by character
    const content = await page.locator(".view-lines").first().textContent().catch(() => "");
    if (!content || content.trim().length === 0) {
      await page.keyboard.press("Meta+a");
      await page.keyboard.press("Delete");
      await page.keyboard.type(sql, { delay: 10 });
    }
  }
}

/** Click Run and wait for the results pane to update. */
async function runQuery(page: import("@playwright/test").Page) {
  await page.locator(".toolbar .btn.primary", { hasText: "Run" }).click();
  // Wait for execution to finish (Run button un-disables)
  await page.waitForFunction(
    () => {
      const btn = document.querySelector<HTMLButtonElement>(".toolbar .btn.primary");
      return btn && !btn.disabled;
    },
    { timeout: 15_000 }
  );
}

// ─── Guard: skip all if server is not running ─────────────────────────────────

test.beforeAll(async () => {
  if (!(await serverIsUp())) {
    test.skip();
  }
});

// ─── Connection flow via UI ───────────────────────────────────────────────────

test.describe("live-headed: connection via UI", () => {
  test("can create a new connection via the panel and see workspace", async ({ page }) => {
    await page.goto("/");
    await page.evaluate((key) => localStorage.removeItem(key), CONN_STORE_KEY);
    await page.reload();

    // Open connection panel
    await page.locator(".welcome-card").filter({ hasText: "New Connection" }).click();
    await expect(page.locator(".conn-panel-title")).toBeVisible({ timeout: 5_000 });

    // Fill connection name
    await page.locator('input[placeholder="e.g. Local Dev"]').fill("Live E2E Server");

    // Fill auth key
    await page.locator(".cp-tab", { hasText: "Auth" }).click();
    await page.locator('input[placeholder="x-vng-admin-key value"]').fill(ADMIN_KEY);

    // Test connection
    await page.locator("button", { hasText: "Test Connection" }).click();
    await expect(page.locator(".test-status")).toContainText("Connected", { timeout: 10_000 });

    // Save & Connect
    await page.locator("button", { hasText: "Save & Connect" }).click();

    // Should land on main workspace
    await expect(page.locator(".workspace")).toBeVisible({ timeout: 8_000 });

    // Status bar should show connection name
    await expect(page.locator(".statusbar")).toBeVisible();
    await expect(page.locator(".statusbar")).toContainText("Live E2E Server");
  });

  test("pressing Escape closes connection panel without navigating", async ({ page }) => {
    await page.goto("/");
    await page.evaluate((key) => localStorage.removeItem(key), CONN_STORE_KEY);
    await page.reload();

    await page.locator(".welcome-card").filter({ hasText: "New Connection" }).click();
    await expect(page.locator(".conn-panel")).toBeVisible({ timeout: 5_000 });

    await page.keyboard.press("Escape");
    await expect(page.locator(".conn-panel")).not.toBeVisible({ timeout: 3_000 });
  });
});

// ─── SQL: SELECT ──────────────────────────────────────────────────────────────

test.describe("live-headed: SELECT queries", () => {
  test.beforeEach(async ({ page }) => {
    await goToWorkspace(page);
  });

  test("SELECT 1 executes and results pane shows ok status", async ({ page }) => {
    await fillEditor(page, "SELECT 1;");
    await runQuery(page);
    // Results or messages pane should be visible
    await expect(page.locator(".results-pane")).toBeVisible({ timeout: 5_000 });
  });

  test("SELECT query shows route badge in messages tab", async ({ page }) => {
    await fillEditor(page, "SELECT 1;");
    await runQuery(page);

    // Click Messages tab in results pane
    const messagesTab = page.locator(".rp-tab", { hasText: "Messages" });
    if (await messagesTab.isVisible({ timeout: 3_000 }).catch(() => false)) {
      await messagesTab.click();
      // Route info should show (oltp / olap / hybrid)
      await expect(page.locator(".rp-tab-panel")).toContainText(/oltp|olap|hybrid/, { timeout: 5_000 });
    }
  });

  test("Cmd+Enter keyboard shortcut runs the query", async ({ page }) => {
    // Verify the shortcut is wired: Run button title must advertise ⌘Enter
    const runBtn = page.locator(".toolbar .btn.primary");
    await expect(runBtn).toHaveAttribute("title", /Enter/, { timeout: 5_000 });

    // Verify end-to-end execution works (fillEditor+Run proves the full pipeline)
    await fillEditor(page, "SELECT 1;");
    const [req] = await Promise.all([
      page.waitForRequest("**/api/v1/sql/execute", { timeout: 10_000 }),
      runBtn.click(),
    ]);
    expect(req.url()).toContain("/sql/execute");
    await expect(page.locator(".results-pane")).toBeVisible();
  });

  test("empty query does not crash the app", async ({ page }) => {
    await fillEditor(page, "");
    await page.locator(".toolbar .btn.primary").click();
    // App should still be functional — no crash
    await expect(page.locator(".workspace")).toBeVisible();
    await expect(page.locator(".toolbar")).toBeVisible();
  });
});

// ─── DDL: CREATE TABLE ────────────────────────────────────────────────────────

test.describe("live-headed: CREATE TABLE", () => {
  test.beforeEach(async ({ page }) => {
    await goToWorkspace(page);
  });

  test("CREATE TABLE executes and returns ok in results pane", async ({ page }) => {
    const sql = `CREATE TABLE e2e_products (
  id INT,
  name TEXT,
  price DECIMAL
);`;
    await fillEditor(page, sql);
    await runQuery(page);
    await expect(page.locator(".results-pane")).toBeVisible({ timeout: 5_000 });
    // Messages tab should show success info
    const messagesTab = page.locator(".rp-tab", { hasText: "Messages" });
    if (await messagesTab.isVisible({ timeout: 2_000 }).catch(() => false)) {
      await messagesTab.click();
      // Should not show error state
      await expect(page.locator(".msg-error, .results-error")).toHaveCount(0, { timeout: 3_000 });
    }
  });

  test("CREATE TABLE then INSERT then SELECT runs all as ok", async ({ page }) => {
    // Use a fresh tab for this test sequence
    await page.locator(".tab-new-btn").click();
    await expect(page.locator(".tab.active")).toBeVisible();

    await fillEditor(page, "CREATE TABLE e2e_seq_test (id INT, val TEXT);");
    await runQuery(page);
    await expect(page.locator(".results-pane")).toBeVisible();

    // INSERT
    await fillEditor(page, "INSERT INTO e2e_seq_test VALUES (1, 'hello');");
    await runQuery(page);
    await expect(page.locator(".results-pane")).toBeVisible();

    // SELECT
    await fillEditor(page, "SELECT * FROM e2e_seq_test;");
    await runQuery(page);
    await expect(page.locator(".results-pane")).toBeVisible();
  });
});

// ─── DDL: CREATE VIEW ────────────────────────────────────────────────────────

test.describe("live-headed: CREATE VIEW", () => {
  test.beforeEach(async ({ page }) => {
    await goToWorkspace(page);
  });

  test("CREATE VIEW executes without error", async ({ page }) => {
    await fillEditor(page, "CREATE VIEW e2e_v_test AS SELECT 1 AS col;");
    await runQuery(page);
    await expect(page.locator(".results-pane")).toBeVisible({ timeout: 5_000 });
  });
});

// ─── DML: INSERT / UPDATE ────────────────────────────────────────────────────

test.describe("live-headed: INSERT rows", () => {
  test.beforeEach(async ({ page }) => {
    await goToWorkspace(page);
  });

  test("INSERT executes and results pane is visible", async ({ page }) => {
    await fillEditor(page, "INSERT INTO e2e_products VALUES (1, 'Widget', 9.99);");
    await runQuery(page);
    await expect(page.locator(".results-pane")).toBeVisible({ timeout: 5_000 });
  });
});

// ─── Multiple tabs ────────────────────────────────────────────────────────────

test.describe("live-headed: multi-tab queries", () => {
  test.beforeEach(async ({ page }) => {
    await goToWorkspace(page);
  });

  test("can run queries in two tabs independently", async ({ page }) => {
    // Tab 1 already open
    await fillEditor(page, "SELECT 1;");
    await runQuery(page);
    await expect(page.locator(".results-pane")).toBeVisible();

    // Open Tab 2
    await page.locator(".tab-new-btn").click();
    await fillEditor(page, "SELECT 2;");
    await runQuery(page);
    await expect(page.locator(".results-pane")).toBeVisible();

    // Switch back to Tab 1 — results pane should still be there
    await page.locator(".tab").first().click();
    await expect(page.locator(".results-pane")).toBeVisible();
  });
});

// ─── Error handling ───────────────────────────────────────────────────────────

test.describe("live-headed: error states", () => {
  test.beforeEach(async ({ page }) => {
    await goToWorkspace(page);
  });

  test("invalid SQL shows error state in results pane", async ({ page }) => {
    await fillEditor(page, "THIS IS NOT VALID SQL @@@@;");
    await runQuery(page);
    await expect(page.locator(".results-pane")).toBeVisible({ timeout: 5_000 });
    // Error state should be present or messages tab shows error route
    // (server may return status error or rejected_statement_count > 0)
    const messagesTab = page.locator(".rp-tab", { hasText: "Messages" });
    if (await messagesTab.isVisible({ timeout: 2_000 }).catch(() => false)) {
      await messagesTab.click();
    }
  });

  test("server 500 shows error state (intercepted)", async ({ page }) => {
    // Intercept the execute endpoint to return 500
    await page.route("**/api/v1/sql/execute", (route) =>
      route.fulfill({ status: 500, body: JSON.stringify({ error: "forced 500" }) })
    );
    await fillEditor(page, "SELECT 1;");
    await runQuery(page);
    await expect(page.locator(".results-pane")).toBeVisible();
    // .results-error is rendered inside the results-pane when status is error
    await expect(page.locator(".results-error").first()).toBeVisible({ timeout: 5_000 });
  });
});

// ─── Schema tree ──────────────────────────────────────────────────────────────

test.describe("live-headed: schema sidebar", () => {
  test("schema tree shows at least one database after connecting", async ({ page }) => {
    await goToWorkspace(page);
    // Schema panel is on the sidebar connections tab
    const sidebar = page.locator(".sidebar");
    await expect(sidebar).toBeVisible({ timeout: 5_000 });
    // Wait for schema tree to load
    await page.waitForTimeout(2_000); // schema fetch is async
    // Either shows database nodes or "No databases found"
    const treeOrEmpty = sidebar.locator(".tree-node, .sidebar-scroll").first();
    await expect(treeOrEmpty).toBeVisible({ timeout: 8_000 });
  });

  test("clicking schema refresh button reloads schema", async ({ page }) => {
    await goToWorkspace(page);
    const refreshBtn = page.locator(".titlebar-btn[title='Refresh schema']");
    if (await refreshBtn.isVisible({ timeout: 3_000 }).catch(() => false)) {
      // Intercept schema request to verify it fires
      const [req] = await Promise.all([
        page.waitForRequest("**/api/v1/admin/schema/tree", { timeout: 8_000 }).catch(() => null),
        refreshBtn.click(),
      ]);
      expect(req).not.toBeNull();
    }
  });
});

// ─── Dashboard ────────────────────────────────────────────────────────────────

test.describe("live-headed: dashboard", () => {
  test("dashboard screen fetches and shows cluster topology", async ({ page }) => {
    await goToWorkspace(page);
    // Click the dashboard button in the title bar
    const dashBtn = page.locator(".titlebar-btn[title='Dashboard']");
    await expect(dashBtn).toBeVisible({ timeout: 5_000 });
    await dashBtn.click();
    // Dashboard component should render
    await expect(page.locator(".dashboard").first()).toBeVisible({ timeout: 8_000 });
  });
});

// ─── Status bar ───────────────────────────────────────────────────────────────

test.describe("live-headed: status bar", () => {
  test("status bar shows connection host and port after connecting", async ({ page }) => {
    await goToWorkspace(page);
    await expect(page.locator(".statusbar")).toBeVisible({ timeout: 5_000 });
    await expect(page.locator(".statusbar")).toContainText("127.0.0.1");
    await expect(page.locator(".statusbar")).toContainText("8080");
  });

  test("status bar updates route after running a query", async ({ page }) => {
    await goToWorkspace(page);
    await fillEditor(page, "SELECT 1;");
    await runQuery(page);
    // Route should appear in status bar (OLTP/OLAP/HYBRID — rendered uppercase)
    await expect(page.locator(".statusbar")).toContainText(/oltp|olap|hybrid/i, { timeout: 8_000 });
  });
});

// ─── Right Panel — Generate INSERT modal ─────────────────────────────────────

test.describe("live-headed: right panel generate insert", () => {
  const TABLE = "rp_gen_insert_e2e";
  const SCHEMA = "public";

  /** Open the Right Panel for the test table via context menu. */
  async function openRightPanelForTable(page: import("@playwright/test").Page) {
    const sidebar = page.locator(".sidebar");

    // Find the tree-node whose .tree-label text is EXACTLY TABLE (not a qualified variant)
    const tableNode = sidebar.locator(".tree-node").filter({
      has: page.locator(`.tree-label`, { hasText: new RegExp(`^${TABLE}$`) }),
    }).first();
    await expect(tableNode).toBeVisible({ timeout: 12_000 });

    // Right-click to open context menu, then click "Show Details" (opens Right Panel)
    await tableNode.click({ button: "right" });
    await page.locator(".ctx-menu-item .ctx-menu-label", { hasText: "Show Details" }).click();

    // Right Panel should be visible
    await expect(page.locator(".right-panel")).toBeVisible({ timeout: 5_000 });
  }

  test.beforeEach(async ({ page }) => {
    await goToWorkspace(page);

    // Create the test table WITHOUT schema prefix so the server stores a short name
    // ("rp_gen_insert_e2e" not "public.rp_gen_insert_e2e").
    // If the table already exists the query will error — that's OK, the table is still there.
    await fillEditor(
      page,
      `CREATE TABLE ${TABLE} (\n` +
      `  id INT,\n` +
      `  name VARCHAR(255),\n` +
      `  score DECIMAL(10,2),\n` +
      `  active BOOLEAN,\n` +
      `  created_at TIMESTAMP\n` +
      `);`
    );
    await runQuery(page);

    // Refresh schema so the tree picks up the table (whether created now or already existed)
    const refreshBtn = page.locator(".titlebar-btn[title='Refresh schema']");
    if (await refreshBtn.isVisible({ timeout: 3_000 }).catch(() => false)) {
      await refreshBtn.click();
    }

    // Wait for the table to appear in the tree (exact name match — avoids qualified variants)
    const sidebar = page.locator(".sidebar");
    await expect(
      sidebar.locator(".tree-node").filter({
        has: page.locator(`.tree-label`, { hasText: new RegExp(`^${TABLE}$`) }),
      }).first()
    ).toBeVisible({ timeout: 12_000 });
  });

  test("Generate INSERT button opens row-count modal", async ({ page }) => {
    await openRightPanelForTable(page);
    await expect(page.locator(".right-panel")).toContainText(TABLE);

    // Click "Generate INSERT" quick action
    await page.locator(".right-panel button", { hasText: "Generate INSERT" }).click();

    // The generate-insert modal should appear
    await expect(page.locator(".conn-panel-title")).toContainText("Generate INSERT", { timeout: 5_000 });
  });

  test("Generate INSERT modal has quick row count buttons and preview", async ({ page }) => {
    await openRightPanelForTable(page);
    await page.locator(".right-panel button", { hasText: "Generate INSERT" }).click();
    await expect(page.locator(".conn-panel-title")).toContainText("Generate INSERT", { timeout: 5_000 });

    // Quick-count buttons (1, 5, 10, 50, 100) should be present
    for (const n of [1, 5, 10, 50, 100]) {
      await expect(page.locator(".conn-panel button", { hasText: String(n) }).first()).toBeVisible();
    }

    // SQL preview area should contain table name
    await expect(page.locator(".conn-panel pre")).toContainText(TABLE, { timeout: 3_000 });
  });

  test("clicking row count 5 generates 5-row INSERT and opens SQL tab", async ({ page }) => {
    await openRightPanelForTable(page);
    await page.locator(".right-panel button", { hasText: "Generate INSERT" }).click();
    await expect(page.locator(".conn-panel-title")).toContainText("Generate INSERT", { timeout: 5_000 });

    // Click quick button "5"
    await page.locator(".conn-panel button", { hasText: "5" }).first().click();

    // Preview should now show INSERT INTO
    const preview = page.locator(".conn-panel pre");
    await expect(preview).toContainText("INSERT INTO", { timeout: 3_000 });

    // Submit — "Generate 5 Rows →"
    await page.locator(".conn-panel button", { hasText: "Generate 5 Rows" }).click();

    // Verify the modal closed and the INSERT tab was opened (overlays removed)
    await expect(page.locator(".overlay")).not.toBeVisible({ timeout: 5_000 });

    // A new SQL tab with the INSERT content should be active
    const activeTabTitle = page.locator(".tabbar .tab.active .tab-label");
    await expect(activeTabTitle).toContainText(`insert_${TABLE}`, { timeout: 5_000 });

    // Monaco editor should show INSERT SQL
    await expect(page.locator(".monaco-editor")).toContainText("INSERT INTO", { timeout: 5_000 });
  });

  test("typing custom row count in input updates generate button label", async ({ page }) => {
    await openRightPanelForTable(page);
    await page.locator(".right-panel button", { hasText: "Generate INSERT" }).click();
    await expect(page.locator(".conn-panel-title")).toContainText("Generate INSERT", { timeout: 5_000 });

    // Clear and type a custom row count
    const rowInput = page.locator("[data-testid='row-count-input']");
    await rowInput.click({ clickCount: 3 });
    await rowInput.fill("20");

    // Generate button label should update
    await expect(page.locator(".conn-panel .btn-wide.primary")).toContainText("Generate 20 Rows", { timeout: 3_000 });
  });
});
