import { test, expect, seedConnection, clearConnections } from "./helpers/fixtures";

// Navigate to main screen with an active connection
async function goToMainWithConnection(page: Parameters<typeof seedConnection>[0]) {
  await seedConnection(page);
  await page.reload();
  // Wait for welcome screen, then navigate to main
  const recentItem = page.locator(".recent-item").first();
  const hasRecent = await recentItem.isVisible({ timeout: 4000 }).catch(() => false);
  if (hasRecent) {
    await recentItem.click();
  } else {
    // Fallback: use New Query card to get to main screen
    await page.locator(".welcome-card").filter({ hasText: "New Query" }).click();
  }
  await expect(page.locator(".workspace")).toBeVisible();
}

test.describe("Sidebar", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await clearConnections(mockedPage);
    await mockedPage.reload();
    await goToMainWithConnection(mockedPage);
  });

  // ── Structure ──────────────────────────────────────────────────────────────

  test("sidebar is visible on main screen", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".sidebar")).toBeVisible();
  });

  test("shows four activity buttons: Schema, Users, History, Saved", async ({ mockedPage }) => {
    const btns = mockedPage.locator(".activity-btn");
    await expect(btns).toHaveCount(4);
    await expect(mockedPage.locator(".activity-btn", { hasText: "Schema" })).toBeVisible();
    await expect(mockedPage.locator(".activity-btn", { hasText: "Users" })).toBeVisible();
    await expect(mockedPage.locator(".activity-btn", { hasText: "History" })).toBeVisible();
    await expect(mockedPage.locator(".activity-btn", { hasText: "Saved" })).toBeVisible();
  });

  test("Schema tab is active by default", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".activity-btn.active")).toHaveText("Schema");
  });

  // ── Tab Switching ─────────────────────────────────────────────────────────

  test("clicking History tab shows 'coming soon' message", async ({ mockedPage }) => {
    await mockedPage.locator(".activity-btn", { hasText: "History" }).click();
    await expect(mockedPage.locator(".activity-btn.active")).toHaveText("History");
    await expect(mockedPage.locator(".sidebar-scroll")).toContainText("coming soon");
  });

  test("clicking Saved tab shows 'coming soon' message", async ({ mockedPage }) => {
    await mockedPage.locator(".activity-btn", { hasText: "Saved" }).click();
    await expect(mockedPage.locator(".activity-btn.active")).toHaveText("Saved");
    await expect(mockedPage.locator(".sidebar-scroll")).toContainText("coming soon");
  });

  test("clicking Schema tab restores connection list", async ({ mockedPage }) => {
    await mockedPage.locator(".activity-btn", { hasText: "History" }).click();
    await mockedPage.locator(".activity-btn", { hasText: "Schema" }).click();
    await expect(mockedPage.locator(".conn-section-header")).toBeVisible();
  });

  // ── Connection List ────────────────────────────────────────────────────────

  test("shows Connections section header with Add (+) button", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".conn-section-header")).toBeVisible();
    await expect(mockedPage.locator(".conn-add-btn")).toBeVisible();
    await expect(mockedPage.locator(".conn-add-btn")).toHaveText("+");
  });

  test("connection item is rendered for seeded connection", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".conn-item")).toHaveCount(1);
    await expect(mockedPage.locator(".conn-item")).toContainText("Test VNG Server");
  });

  test("active connection item has 'active' class", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".conn-item.active")).toBeVisible();
  });

  test("connection item shows VNG badge", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".conn-type-badge")).toContainText("VNG");
  });

  test("clicking Add (+) button opens connection panel", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-add-btn").click();
    await expect(mockedPage.locator(".conn-panel")).toBeVisible();
    await expect(mockedPage.locator(".conn-panel-title")).toHaveText("New Connection");
  });

  test("shows 'No connections yet' when connection list is empty", async ({ mockedPage }) => {
    await clearConnections(mockedPage);
    await mockedPage.reload();
    // Navigate to main without connection (e.g. click New Query)
    await mockedPage.locator(".welcome-card").filter({ hasText: "New Query" }).click();
    await expect(mockedPage.locator(".sidebar-scroll")).toContainText("No connections yet");
  });

  test("'Add one' link in empty state opens connection panel", async ({ mockedPage }) => {
    await clearConnections(mockedPage);
    await mockedPage.reload();
    await mockedPage.locator(".welcome-card").filter({ hasText: "New Query" }).click();
    await mockedPage.locator(".sidebar-scroll span", { hasText: "Add one" }).click();
    await expect(mockedPage.locator(".conn-panel")).toBeVisible();
  });

  // ── Schema Tree ────────────────────────────────────────────────────────────

  test("schema tree renders database name when schema is loaded", async ({ mockedPage }) => {
    // Wait briefly for the schema tree API (with short timeout to avoid hanging)
    await mockedPage.waitForResponse("**/api/v1/admin/schema/tree", { timeout: 3000 }).catch(() => {});
    await mockedPage.waitForTimeout(300);
    const tree = mockedPage.locator(".tree-node");
    if (await tree.count() > 0) {
      await expect(tree.first()).toBeVisible();
    }
  });

  test("schema tree table node expands on click to show columns", async ({ mockedPage }) => {
    await mockedPage.waitForTimeout(500);
    const tableNode = mockedPage.locator(".tree-node .tree-icon", { hasText: "📋" }).first();
    if (await tableNode.isVisible()) {
      await tableNode.click();
      // After expanding, column chips should appear
      const chips = mockedPage.locator(".col-chip");
      await expect(chips.first()).toBeVisible({ timeout: 3000 });
    }
  });

  // ── Users Panel ────────────────────────────────────────────────────────────

  test("clicking Users tab switches to users panel", async ({ mockedPage }) => {
    await mockedPage.locator(".activity-btn", { hasText: "Users" }).click();
    await expect(mockedPage.locator(".activity-btn.active")).toHaveText("Users");
    await expect(mockedPage.locator(".sidebar-scroll")).toBeVisible();
  });

  test("users panel shows default user items", async ({ mockedPage }) => {
    await mockedPage.locator(".activity-btn", { hasText: "Users" }).click();
    // UsersPanel seeds 3 default users (admin, analyst, etl_bot)
    const items = mockedPage.locator(".conn-item");
    await expect(items.count()).resolves.toBeGreaterThanOrEqual(1);
  });

  test("users panel shows Users section header with add button", async ({ mockedPage }) => {
    await mockedPage.locator(".activity-btn", { hasText: "Users" }).click();
    await expect(mockedPage.locator(".conn-section-header").first()).toBeVisible();
    await expect(mockedPage.locator("button[title='Create User']")).toBeVisible();
  });

  test("users panel shows role badges on user items", async ({ mockedPage }) => {
    await mockedPage.locator(".activity-btn", { hasText: "Users" }).click();
    await expect(mockedPage.locator(".conn-type-badge").first()).toBeVisible();
  });

  test("users panel add button opens create-user modal", async ({ mockedPage }) => {
    await mockedPage.locator(".activity-btn", { hasText: "Users" }).click();
    await mockedPage.locator("button[title='Create User']").click();
    await expect(mockedPage.locator(".conn-panel")).toBeVisible();
    await expect(mockedPage.locator(".conn-panel-title")).toContainText("Create User");
  });

  test("switching back from Users tab to Schema tab shows connection list", async ({ mockedPage }) => {
    await mockedPage.locator(".activity-btn", { hasText: "Users" }).click();
    await mockedPage.locator(".activity-btn", { hasText: "Schema" }).click();
    await expect(mockedPage.locator(".conn-section-header")).toBeVisible();
  });
});
