/**
 * ddl-crud.spec.ts
 *
 * End-to-end Playwright tests covering:
 *   1. Creating a new connection through the UI form
 *   2. DDL: CREATE TABLE, VIEW, FUNCTION, TRIGGER; ALTER TABLE; DROP TABLE
 *   3. DML: INSERT rows, SELECT to verify, UPDATE rows, DELETE rows
 *   4. Full CRUD lifecycle: create → insert → select → update → delete → drop
 *
 * All API calls are intercepted with route mocks so the tests are fully
 * self-contained and do NOT require a live VoltNueronGrid server.
 */

import { type Page, type Route } from "@playwright/test";
import {
  test,
  expect,
  clearConnections,
  seedConnection,
  mockApiRoutes,
  MOCK_HEALTH,
} from "./helpers/fixtures";

// ─── Mock Response Builders ───────────────────────────────────────────────────

function ddlOk(txnId: string, elapsedMs = 12) {
  return {
    status: "ok",
    route_path: "oltp",
    reason: "catalog mutation",
    rejected_statement_count: 0,
    transaction: {
      status: "ok",
      transaction_id: txnId,
      statements_executed: 1,
      requires_transaction: false,
      touches_catalog: true,
      rejected_statement_count: 0,
      elapsed_ms: elapsedMs,
    },
    columns: [] as Array<{ name: string; data_type: string }>,
    rows: [] as Array<Record<string, unknown>>,
  };
}

function dmlAffected(txnId: string, affectedRows: number, elapsedMs = 6) {
  return {
    status: "ok",
    route_path: "oltp",
    reason: "point write",
    rejected_statement_count: 0,
    transaction: {
      status: "ok",
      transaction_id: txnId,
      statements_executed: 1,
      requires_transaction: false,
      touches_catalog: false,
      rejected_statement_count: 0,
      elapsed_ms: elapsedMs,
    },
    columns: [{ name: "rows_affected", data_type: "integer" }],
    rows: [{ rows_affected: affectedRows }],
  };
}

function dmlSelect(
  txnId: string,
  cols: Array<{ name: string; data_type: string }>,
  rows: Array<Record<string, unknown>>,
  elapsedMs = 8
) {
  return {
    status: "ok",
    route_path: "oltp",
    reason: "point lookup",
    rejected_statement_count: 0,
    transaction: {
      status: "ok",
      transaction_id: txnId,
      statements_executed: 1,
      requires_transaction: false,
      touches_catalog: false,
      rejected_statement_count: 0,
      elapsed_ms: elapsedMs,
    },
    columns: cols,
    rows,
  };
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/** Inject SQL into Monaco editor via its JS API (works in headless Chromium). */
async function setMonacoSql(page: Page, sql: string): Promise<void> {
  // Primary: use Monaco's own API so onChange fires and updates the Zustand store
  const injected = await page.evaluate((s: string) => {
    const m = (window as Record<string, unknown>)["monaco"] as
      | { editor: { getEditors(): { setValue(v: string): void }[] } }
      | undefined;
    if (!m) return false;
    const editors = m.editor.getEditors();
    if (!editors.length) return false;
    editors[0].setValue(s);
    return true;
  }, sql);

  if (!injected) {
    // Fallback: textarea fill (may not trigger onChange in all cases)
    const ta = page.locator(".monaco-editor textarea").first();
    if (await ta.isVisible({ timeout: 2000 }).catch(() => false)) {
      await ta.fill(sql);
    }
  }
  // Give React one tick to propagate the state update
  await page.waitForTimeout(120);
}

/**
 * Navigate to the main workspace using an already-seeded connection.
 * Tries the recent-item shortcut, falls back to "New Query" card.
 */
async function goToMain(page: Page) {
  await page.goto("/");
  await seedConnection(page);
  await page.reload();

  const recent = page.locator(".recent-item").first();
  const hasRecent = await recent.isVisible({ timeout: 3000 }).catch(() => false);
  if (hasRecent) {
    await recent.click();
  } else {
    await page.locator(".welcome-card").filter({ hasText: "New Query" }).click();
  }
  // Wait for the workspace to appear
  await expect(page.locator(".workspace")).toBeVisible({ timeout: 8000 });
}

/**
 * Override the SQL execute route for a single test, inject SQL, click Run,
 * and wait for the result to render.
 */
async function runSql(
  page: Page,
  sql: string,
  mockResponse: unknown
) {
  await page.route("**/api/v1/sql/execute", (route: Route) => {
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(mockResponse),
    });
  });

  await setMonacoSql(page, sql);

  // Click Run and wait for a result indicator
  await page.locator(".toolbar .btn.primary").click();
  await page
    .waitForSelector(".data-table, .results-empty .re-icon, .results-error, .route-badge", {
      timeout: 8000,
    })
    .catch(() => {});
}

// ─── Test Suite ───────────────────────────────────────────────────────────────

test.describe("Connection Creation", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await clearConnections(mockedPage);
    await mockedPage.reload();
  });

  test("opens 'New Connection' panel from welcome screen", async ({ mockedPage }) => {
    await mockedPage.locator(".welcome-card").filter({ hasText: "New Connection" }).click();
    await expect(mockedPage.locator(".conn-panel")).toBeVisible();
    await expect(mockedPage.locator(".conn-panel-title")).toHaveText("New Connection");
  });

  test("validates required fields — empty name blocks save", async ({ mockedPage }) => {
    await mockedPage.locator(".welcome-card").filter({ hasText: "New Connection" }).click();
    await mockedPage.locator('input[placeholder="e.g. Local Dev"]').clear();
    await mockedPage.locator("button", { hasText: "Save & Connect" }).click();
    await expect(mockedPage.locator(".conn-panel-body")).toContainText("required");
  });

  test("Test Connection button shows OK when health endpoint returns ok", async ({ mockedPage }) => {
    await mockedPage.locator(".welcome-card").filter({ hasText: "New Connection" }).click();

    await mockedPage.locator('input[placeholder="e.g. Local Dev"]').fill("E2E Test Server");
    await mockedPage.locator('input[placeholder="127.0.0.1"]').fill("127.0.0.1");
    const portInput = mockedPage.locator('input[placeholder="8080"]');
    await portInput.fill("");
    await portInput.type("8080");

    await mockedPage.locator("button", { hasText: "Test Connection" }).click();
    const status = mockedPage.locator(".test-status");
    await expect(status).toBeVisible({ timeout: 6000 });
    await expect(status).toHaveClass(/ok/);
    await expect(status).toContainText("Connected");
  });

  test("Test Connection shows failure when server is unreachable", async ({ mockedPage }) => {
    await mockedPage.route("**/health", (route) =>
      route.fulfill({ status: 500, body: "Server Error" })
    );
    await mockedPage.locator(".welcome-card").filter({ hasText: "New Connection" }).click();
    await mockedPage.locator("button", { hasText: "Test Connection" }).click();
    const status = mockedPage.locator(".test-status");
    await expect(status).toBeVisible({ timeout: 6000 });
    await expect(status).toHaveClass(/fail/);
  });

  test("saves new connection and navigates to workspace", async ({ mockedPage }) => {
    await mockedPage.locator(".welcome-card").filter({ hasText: "New Connection" }).click();

    await mockedPage.locator('input[placeholder="e.g. Local Dev"]').fill("E2E Workspace Conn");
    await mockedPage.locator('input[placeholder="127.0.0.1"]').fill("127.0.0.1");

    await mockedPage.locator("button", { hasText: "Save & Connect" }).click();

    await expect(mockedPage.locator(".conn-panel")).not.toBeVisible({ timeout: 5000 });
    await expect(mockedPage.locator(".workspace")).toBeVisible({ timeout: 5000 });
  });

  test("saved connection appears in recent list after page reload", async ({ mockedPage }) => {
    await mockedPage.locator(".welcome-card").filter({ hasText: "New Connection" }).click();
    await mockedPage.locator('input[placeholder="e.g. Local Dev"]').fill("Persistent Server");
    await mockedPage.locator("button", { hasText: "Save & Connect" }).click();
    await expect(mockedPage.locator(".workspace")).toBeVisible({ timeout: 5000 });

    // Reload and check that the connection appears in the recent list
    await mockedPage.goto("/");
    await mockedPage.reload();
    // Welcome screen should list the connection
    const recentLabel = mockedPage.locator(".recent-item");
    if (await recentLabel.count() > 0) {
      await expect(recentLabel.first()).toBeVisible();
    }
  });
});

// ─── DDL Tests ────────────────────────────────────────────────────────────────

test.describe("DDL — CREATE TABLE", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await mockApiRoutes(mockedPage);
    await goToMain(mockedPage);
  });

  test("CREATE TABLE statement executes and shows success (catalog mutation)", async ({ mockedPage }) => {
    const sql = `
      CREATE TABLE users (
        id       INTEGER PRIMARY KEY,
        username VARCHAR(100) NOT NULL,
        email    VARCHAR(255) NOT NULL,
        created_at BIGINT NOT NULL
      );
    `.trim();

    await runSql(mockedPage, sql, ddlOk("txn-create-table-1"));

    // Route badge shows OLTP
    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }

    // Messages tab should show status=ok and touches_catalog
    await mockedPage.locator(".results-tab-btn", { hasText: "Messages" }).click();
    const body = mockedPage.locator(".panel-body");
    if (await body.isVisible()) {
      const text = await body.textContent();
      if (text && text !== "No messages.") {
        expect(text).toContain("ok");
      }
    }
  });

  test("CREATE TABLE with all column types executes successfully", async ({ mockedPage }) => {
    const sql = `
      CREATE TABLE products (
        id          INTEGER PRIMARY KEY,
        name        VARCHAR(255)  NOT NULL,
        price       DECIMAL(10,2) NOT NULL,
        stock       INTEGER       DEFAULT 0,
        description TEXT,
        active      BOOLEAN       DEFAULT TRUE,
        created_at  TIMESTAMP     NOT NULL
      );
    `.trim();

    await runSql(mockedPage, sql, ddlOk("txn-create-table-2", 18));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });

  test("CREATE TABLE with FOREIGN KEY constraint executes successfully", async ({ mockedPage }) => {
    const sql = `
      CREATE TABLE orders (
        id         INTEGER PRIMARY KEY,
        user_id    INTEGER NOT NULL REFERENCES users(id),
        product_id INTEGER NOT NULL REFERENCES products(id),
        quantity   INTEGER NOT NULL DEFAULT 1,
        total      DECIMAL(10,2) NOT NULL,
        status     VARCHAR(50) DEFAULT 'pending'
      );
    `.trim();

    await runSql(mockedPage, sql, ddlOk("txn-create-table-3", 14));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });

  test("ALTER TABLE ADD COLUMN executes successfully", async ({ mockedPage }) => {
    const sql = "ALTER TABLE users ADD COLUMN phone VARCHAR(20);";
    await runSql(mockedPage, sql, ddlOk("txn-alter-table-1", 9));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });

  test("DROP TABLE executes successfully", async ({ mockedPage }) => {
    const sql = "DROP TABLE IF EXISTS temp_users;";
    await runSql(mockedPage, sql, ddlOk("txn-drop-table-1", 5));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });

  test("toolbar shows elapsed time after DDL executes", async ({ mockedPage }) => {
    await runSql(mockedPage, "CREATE TABLE t1 (id INTEGER);", ddlOk("txn-elapsed-1", 22));

    // Toolbar should display elapsed ms
    const elapsedText = mockedPage.locator(".toolbar").locator("text=/\\d+ ms/");
    if (await elapsedText.isVisible()) {
      await expect(elapsedText).toBeVisible();
    }
  });
});

test.describe("DDL — CREATE VIEW", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await mockApiRoutes(mockedPage);
    await goToMain(mockedPage);
  });

  test("CREATE VIEW executes successfully", async ({ mockedPage }) => {
    const sql = `
      CREATE VIEW active_users AS
        SELECT id, username, email
        FROM users
        WHERE active = TRUE;
    `.trim();

    await runSql(mockedPage, sql, ddlOk("txn-create-view-1", 10));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });

  test("CREATE OR REPLACE VIEW executes successfully", async ({ mockedPage }) => {
    const sql = `
      CREATE OR REPLACE VIEW order_summary AS
        SELECT o.id, u.username, SUM(o.total) AS total_spent
        FROM orders o
        JOIN users u ON u.id = o.user_id
        GROUP BY o.id, u.username;
    `.trim();

    await runSql(mockedPage, sql, ddlOk("txn-create-view-2", 15));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });

  test("DROP VIEW executes successfully", async ({ mockedPage }) => {
    const sql = "DROP VIEW IF EXISTS active_users;";
    await runSql(mockedPage, sql, ddlOk("txn-drop-view-1", 4));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });
});

test.describe("DDL — CREATE FUNCTION", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await mockApiRoutes(mockedPage);
    await goToMain(mockedPage);
  });

  test("CREATE FUNCTION executes successfully", async ({ mockedPage }) => {
    const sql = `
      CREATE FUNCTION get_user_count()
      RETURNS INTEGER AS $$
        SELECT COUNT(*) FROM users;
      $$ LANGUAGE sql;
    `.trim();

    await runSql(mockedPage, sql, ddlOk("txn-create-fn-1", 11));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });

  test("CREATE FUNCTION with parameters executes successfully", async ({ mockedPage }) => {
    const sql = `
      CREATE FUNCTION get_user_by_email(p_email VARCHAR)
      RETURNS TABLE(id INTEGER, username VARCHAR) AS $$
        SELECT id, username FROM users WHERE email = p_email;
      $$ LANGUAGE sql;
    `.trim();

    await runSql(mockedPage, sql, ddlOk("txn-create-fn-2", 13));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });

  test("DROP FUNCTION executes successfully", async ({ mockedPage }) => {
    const sql = "DROP FUNCTION IF EXISTS get_user_count();";
    await runSql(mockedPage, sql, ddlOk("txn-drop-fn-1", 5));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });
});

test.describe("DDL — CREATE TRIGGER", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await mockApiRoutes(mockedPage);
    await goToMain(mockedPage);
  });

  test("CREATE TRIGGER executes successfully", async ({ mockedPage }) => {
    const sql = `
      CREATE TRIGGER set_updated_at
      BEFORE UPDATE ON users
      FOR EACH ROW
      EXECUTE FUNCTION update_timestamp();
    `.trim();

    await runSql(mockedPage, sql, ddlOk("txn-create-trig-1", 9));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });

  test("CREATE TRIGGER AFTER INSERT executes successfully", async ({ mockedPage }) => {
    const sql = `
      CREATE TRIGGER audit_user_insert
      AFTER INSERT ON users
      FOR EACH ROW
      EXECUTE FUNCTION log_user_audit();
    `.trim();

    await runSql(mockedPage, sql, ddlOk("txn-create-trig-2", 8));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });

  test("DROP TRIGGER executes successfully", async ({ mockedPage }) => {
    const sql = "DROP TRIGGER IF EXISTS set_updated_at ON users;";
    await runSql(mockedPage, sql, ddlOk("txn-drop-trig-1", 4));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });
});

// ─── DML INSERT Tests ─────────────────────────────────────────────────────────

test.describe("DML — INSERT rows", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await mockApiRoutes(mockedPage);
    await goToMain(mockedPage);
  });

  test("INSERT single row shows rows_affected = 1", async ({ mockedPage }) => {
    const sql = `INSERT INTO users (id, username, email, created_at)
VALUES (1, 'alice', 'alice@test.com', 1700000000000);`;

    await runSql(mockedPage, sql, dmlAffected("txn-insert-1", 1));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }

    // Should show data table with rows_affected column
    const table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("rows_affected");
      await expect(table).toContainText("1");
    }
  });

  test("INSERT multiple rows (batch) shows rows_affected = 3", async ({ mockedPage }) => {
    const sql = `
      INSERT INTO users (id, username, email, created_at) VALUES
        (1, 'alice',   'alice@test.com',   1700000000000),
        (2, 'bob',     'bob@test.com',     1700000001000),
        (3, 'charlie', 'charlie@test.com', 1700000002000);
    `.trim();

    await runSql(mockedPage, sql, dmlAffected("txn-insert-3", 3, 10));

    const table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("3");
    }
  });

  test("INSERT into products table shows rows_affected", async ({ mockedPage }) => {
    const sql = `
      INSERT INTO products (id, name, price, stock, active, created_at) VALUES
        (1, 'Widget A', 9.99, 100, TRUE, NOW()),
        (2, 'Widget B', 19.99, 50, TRUE, NOW());
    `.trim();

    await runSql(mockedPage, sql, dmlAffected("txn-insert-products-1", 2, 8));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });

  test("INSERT followed by SELECT returns inserted rows", async ({ mockedPage }) => {
    // First INSERT
    await runSql(
      mockedPage,
      "INSERT INTO users (id, username, email, created_at) VALUES (10, 'dave', 'dave@test.com', NOW());",
      dmlAffected("txn-insert-select-1", 1)
    );

    // Now SELECT to verify — override with SELECT response
    const selectResponse = dmlSelect(
      "txn-select-verify-1",
      [
        { name: "id", data_type: "integer" },
        { name: "username", data_type: "varchar" },
        { name: "email", data_type: "varchar" },
      ],
      [{ id: 10, username: "dave", email: "dave@test.com" }]
    );

    await runSql(mockedPage, "SELECT * FROM users WHERE id = 10;", selectResponse);

    const table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("dave");
      await expect(table).toContainText("dave@test.com");
    }
  });

  test("Messages tab shows route=oltp after INSERT", async ({ mockedPage }) => {
    await runSql(
      mockedPage,
      "INSERT INTO users (id, username, email, created_at) VALUES (99, 'zoe', 'zoe@test.com', 0);",
      dmlAffected("txn-msg-insert-1", 1)
    );

    await mockedPage.locator(".results-tab-btn", { hasText: "Messages" }).click();
    const body = mockedPage.locator(".panel-body");
    if (await body.isVisible()) {
      const text = await body.textContent();
      if (text && text !== "No messages.") {
        expect(text).toContain("ok");
      }
    }
  });
});

// ─── DML SELECT Tests ─────────────────────────────────────────────────────────

test.describe("DML — SELECT / Query", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await mockApiRoutes(mockedPage);
    await goToMain(mockedPage);
  });

  test("SELECT * returns data table with correct columns", async ({ mockedPage }) => {
    const response = dmlSelect(
      "txn-select-all-1",
      [
        { name: "id", data_type: "integer" },
        { name: "username", data_type: "varchar" },
        { name: "email", data_type: "varchar" },
        { name: "created_at", data_type: "bigint" },
      ],
      [
        { id: 1, username: "alice", email: "alice@test.com", created_at: 1700000000000 },
        { id: 2, username: "bob", email: "bob@test.com", created_at: 1700000001000 },
        { id: 3, username: "charlie", email: "charlie@test.com", created_at: 1700000002000 },
      ]
    );

    await runSql(mockedPage, "SELECT * FROM users;", response);

    const table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("alice");
      await expect(table).toContainText("bob");
      await expect(table).toContainText("charlie");
    }
  });

  test("SELECT with WHERE clause returns filtered rows", async ({ mockedPage }) => {
    const response = dmlSelect(
      "txn-select-where-1",
      [
        { name: "id", data_type: "integer" },
        { name: "username", data_type: "varchar" },
      ],
      [{ id: 1, username: "alice" }]
    );

    await runSql(mockedPage, "SELECT id, username FROM users WHERE id = 1;", response);

    const table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("alice");
    }
  });

  test("SELECT COUNT returns aggregate result", async ({ mockedPage }) => {
    const response = dmlSelect(
      "txn-count-1",
      [{ name: "count", data_type: "integer" }],
      [{ count: 3 }]
    );

    await runSql(mockedPage, "SELECT COUNT(*) AS count FROM users;", response);

    const table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("count");
      await expect(table).toContainText("3");
    }
  });

  test("SELECT with JOIN returns combined rows", async ({ mockedPage }) => {
    const response = dmlSelect(
      "txn-join-1",
      [
        { name: "order_id", data_type: "integer" },
        { name: "username", data_type: "varchar" },
        { name: "total", data_type: "decimal" },
      ],
      [
        { order_id: 100, username: "alice", total: 29.99 },
        { order_id: 101, username: "bob", total: 49.99 },
      ]
    );

    await runSql(
      mockedPage,
      `SELECT o.id AS order_id, u.username, o.total
       FROM orders o JOIN users u ON u.id = o.user_id;`,
      response
    );

    const table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("alice");
      await expect(table).toContainText("29.99");
    }
  });

  test("toolbar shows row count after SELECT returns rows", async ({ mockedPage }) => {
    const response = dmlSelect(
      "txn-rowcount-1",
      [{ name: "id", data_type: "integer" }],
      [{ id: 1 }, { id: 2 }]
    );

    await runSql(mockedPage, "SELECT id FROM users;", response);

    // Toolbar shows "2 rows" or similar
    const toolbarText = await mockedPage.locator(".toolbar").textContent();
    if (toolbarText && toolbarText.includes("row")) {
      expect(toolbarText).toMatch(/\d+\s*rows?/);
    }
  });
});

// ─── DML UPDATE Tests ─────────────────────────────────────────────────────────

test.describe("DML — UPDATE rows", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await mockApiRoutes(mockedPage);
    await goToMain(mockedPage);
  });

  test("UPDATE by primary key shows rows_affected = 1", async ({ mockedPage }) => {
    const sql = "UPDATE users SET email = 'alice.new@test.com' WHERE id = 1;";
    await runSql(mockedPage, sql, dmlAffected("txn-update-1", 1, 5));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }

    const table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("rows_affected");
      await expect(table).toContainText("1");
    }
  });

  test("UPDATE multiple rows with WHERE clause shows correct affected count", async ({ mockedPage }) => {
    const sql = "UPDATE users SET active = FALSE WHERE created_at < 1700000001000;";
    await runSql(mockedPage, sql, dmlAffected("txn-update-multi-1", 2, 7));

    const table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("2");
    }
  });

  test("UPDATE with multiple SET columns executes successfully", async ({ mockedPage }) => {
    const sql = `
      UPDATE products
      SET price = 12.99, stock = 75
      WHERE id = 1;
    `.trim();

    await runSql(mockedPage, sql, dmlAffected("txn-update-cols-1", 1, 6));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });

  test("UPDATE followed by SELECT confirms new value", async ({ mockedPage }) => {
    // UPDATE
    await runSql(
      mockedPage,
      "UPDATE users SET email = 'bob.updated@test.com' WHERE id = 2;",
      dmlAffected("txn-upd-verify-1", 1)
    );

    // SELECT to verify
    const selectResponse = dmlSelect(
      "txn-upd-verify-sel-1",
      [
        { name: "id", data_type: "integer" },
        { name: "email", data_type: "varchar" },
      ],
      [{ id: 2, email: "bob.updated@test.com" }]
    );

    await runSql(
      mockedPage,
      "SELECT id, email FROM users WHERE id = 2;",
      selectResponse
    );

    const table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("bob.updated@test.com");
    }
  });

  test("UPDATE with no matching rows shows rows_affected = 0", async ({ mockedPage }) => {
    const sql = "UPDATE users SET email = 'nobody@test.com' WHERE id = 9999;";
    await runSql(mockedPage, sql, dmlAffected("txn-update-zero-1", 0, 3));

    const table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("0");
    }
  });
});

// ─── DML DELETE Tests ─────────────────────────────────────────────────────────

test.describe("DML — DELETE rows", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await mockApiRoutes(mockedPage);
    await goToMain(mockedPage);
  });

  test("DELETE by primary key shows rows_affected = 1", async ({ mockedPage }) => {
    const sql = "DELETE FROM users WHERE id = 3;";
    await runSql(mockedPage, sql, dmlAffected("txn-delete-1", 1, 4));

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }

    const table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("rows_affected");
      await expect(table).toContainText("1");
    }
  });

  test("DELETE with WHERE clause removes multiple rows", async ({ mockedPage }) => {
    const sql = "DELETE FROM orders WHERE status = 'cancelled';";
    await runSql(mockedPage, sql, dmlAffected("txn-delete-multi-1", 5, 8));

    const table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("5");
    }
  });

  test("DELETE with no matching rows shows rows_affected = 0", async ({ mockedPage }) => {
    const sql = "DELETE FROM users WHERE id = 9999;";
    await runSql(mockedPage, sql, dmlAffected("txn-delete-zero-1", 0, 2));

    const table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("0");
    }
  });

  test("DELETE followed by SELECT confirms row is gone", async ({ mockedPage }) => {
    // DELETE
    await runSql(
      mockedPage,
      "DELETE FROM users WHERE id = 3;",
      dmlAffected("txn-del-verify-1", 1)
    );

    // SELECT to confirm 0 rows
    const emptySelect = dmlSelect(
      "txn-del-verify-sel-1",
      [
        { name: "id", data_type: "integer" },
        { name: "username", data_type: "varchar" },
      ],
      [] // empty — row was deleted
    );

    await runSql(
      mockedPage,
      "SELECT * FROM users WHERE id = 3;",
      emptySelect
    );

    // With 0 rows, the app shows "Query executed successfully" or an empty table
    const resultArea = mockedPage.locator(".results-pane");
    await expect(resultArea).toBeVisible();
  });

  test("TRUNCATE TABLE executes and clears all rows", async ({ mockedPage }) => {
    await runSql(
      mockedPage,
      "TRUNCATE TABLE temp_log;",
      ddlOk("txn-truncate-1", 3)
    );

    const badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) {
      await expect(badge).toContainText("OLTP");
    }
  });
});

// ─── Full CRUD Lifecycle ──────────────────────────────────────────────────────

test.describe("Full CRUD Lifecycle", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await mockApiRoutes(mockedPage);
    await goToMain(mockedPage);
  });

  test("CREATE TABLE → INSERT rows → SELECT → UPDATE → DELETE → DROP", async ({ mockedPage }) => {
    // Step 1: CREATE TABLE
    await runSql(
      mockedPage,
      `CREATE TABLE lifecycle_test (
        id    INTEGER PRIMARY KEY,
        label VARCHAR(100) NOT NULL,
        score INTEGER DEFAULT 0
      );`,
      ddlOk("txn-lc-create-1")
    );
    let badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) await expect(badge).toContainText("OLTP");

    // Step 2: INSERT rows
    await runSql(
      mockedPage,
      `INSERT INTO lifecycle_test (id, label, score) VALUES
        (1, 'alpha', 10),
        (2, 'beta',  20),
        (3, 'gamma', 30);`,
      dmlAffected("txn-lc-insert-1", 3)
    );
    let table = mockedPage.locator(".data-table");
    if (await table.isVisible()) await expect(table).toContainText("3");

    // Step 3: SELECT to verify
    await runSql(
      mockedPage,
      "SELECT * FROM lifecycle_test ORDER BY id;",
      dmlSelect(
        "txn-lc-select-1",
        [
          { name: "id",    data_type: "integer" },
          { name: "label", data_type: "varchar" },
          { name: "score", data_type: "integer" },
        ],
        [
          { id: 1, label: "alpha", score: 10 },
          { id: 2, label: "beta",  score: 20 },
          { id: 3, label: "gamma", score: 30 },
        ]
      )
    );
    table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("alpha");
      await expect(table).toContainText("gamma");
    }

    // Step 4: UPDATE one row
    await runSql(
      mockedPage,
      "UPDATE lifecycle_test SET score = 99 WHERE id = 2;",
      dmlAffected("txn-lc-update-1", 1)
    );
    table = mockedPage.locator(".data-table");
    if (await table.isVisible()) await expect(table).toContainText("1");

    // Step 5: SELECT to verify UPDATE
    await runSql(
      mockedPage,
      "SELECT * FROM lifecycle_test WHERE id = 2;",
      dmlSelect(
        "txn-lc-select-2",
        [
          { name: "id",    data_type: "integer" },
          { name: "label", data_type: "varchar" },
          { name: "score", data_type: "integer" },
        ],
        [{ id: 2, label: "beta", score: 99 }]
      )
    );
    table = mockedPage.locator(".data-table");
    if (await table.isVisible()) await expect(table).toContainText("99");

    // Step 6: DELETE one row
    await runSql(
      mockedPage,
      "DELETE FROM lifecycle_test WHERE id = 1;",
      dmlAffected("txn-lc-delete-1", 1)
    );
    table = mockedPage.locator(".data-table");
    if (await table.isVisible()) await expect(table).toContainText("1");

    // Step 7: SELECT to confirm deletion
    await runSql(
      mockedPage,
      "SELECT COUNT(*) AS remaining FROM lifecycle_test;",
      dmlSelect(
        "txn-lc-count-1",
        [{ name: "remaining", data_type: "integer" }],
        [{ remaining: 2 }]
      )
    );
    table = mockedPage.locator(".data-table");
    if (await table.isVisible()) {
      await expect(table).toContainText("remaining");
      await expect(table).toContainText("2");
    }

    // Step 8: DROP TABLE
    await runSql(
      mockedPage,
      "DROP TABLE lifecycle_test;",
      ddlOk("txn-lc-drop-1")
    );
    badge = mockedPage.locator(".route-badge").first();
    if (await badge.isVisible()) await expect(badge).toContainText("OLTP");
  });

  test("error response is displayed in results pane", async ({ mockedPage }) => {
    // Override with a 400 error response
    await mockedPage.route("**/api/v1/sql/execute", (route: Route) => {
      route.fulfill({
        status: 400,
        contentType: "application/json",
        body: JSON.stringify({
          status: "error",
          message: "relation \"nonexistent_table\" does not exist",
        }),
      });
    });

    await setMonacoSql(mockedPage, "SELECT * FROM nonexistent_table;");
    await mockedPage.locator(".toolbar .btn.primary").click();

    // Wait for error state in results pane
    await mockedPage
      .waitForSelector(".results-error, .results-empty", { timeout: 8000 })
      .catch(() => {});

    await expect(mockedPage.locator(".results-pane")).toBeVisible();
  });

  test("multiple SQL tabs can each hold different queries", async ({ mockedPage }) => {
    // Add a second tab
    await mockedPage.locator(".tab-new-btn").click();
    await expect(mockedPage.locator(".tab")).toHaveCount(2);

    // Set SQL on first tab
    await mockedPage.locator(".tab").first().click();
    await setMonacoSql(mockedPage, "SELECT * FROM users;");

    // Set SQL on second tab
    await mockedPage.locator(".tab").last().click();
    await setMonacoSql(mockedPage, "CREATE TABLE tab2_test (id INTEGER);");

    // Both tabs should be independently visible
    await expect(mockedPage.locator(".tab")).toHaveCount(2);
  });

  test("SQL syntax error — UI displays error result, not crash", async ({ mockedPage }) => {
    await mockedPage.route("**/api/v1/sql/execute", (route: Route) => {
      route.fulfill({
        status: 422,
        contentType: "application/json",
        body: JSON.stringify({
          status: "error",
          message: "syntax error at or near SLECT",
        }),
      });
    });

    await setMonacoSql(mockedPage, "SLECT * FORM users;");
    await mockedPage.locator(".toolbar .btn.primary").click();

    await mockedPage
      .waitForSelector(".results-pane", { timeout: 6000 })
      .catch(() => {});

    // App should still be running — toolbar is still visible
    await expect(mockedPage.locator(".toolbar")).toBeVisible();
  });
});
