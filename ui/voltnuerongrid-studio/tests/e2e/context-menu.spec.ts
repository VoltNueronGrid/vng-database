/**
 * context-menu.spec.ts
 *
 * End-to-end tests for the right-click context menu system.
 * Covers: opening menus on connection items and schema-tree nodes,
 * menu content, keyboard/click-outside dismissal, and menu-item actions.
 */

import { test, expect, seedConnection, clearConnections } from "./helpers/fixtures";

// ─── Helpers ──────────────────────────────────────────────────────────────────

async function goToMain(page: Parameters<typeof seedConnection>[0]) {
  await seedConnection(page);
  await page.reload();
  const recentItem = page.locator(".recent-item").first();
  const hasRecent = await recentItem.isVisible({ timeout: 4000 }).catch(() => false);
  if (hasRecent) {
    await recentItem.click();
  } else {
    await page.locator(".welcome-card").filter({ hasText: "New Query" }).click();
  }
  await expect(page.locator(".workspace")).toBeVisible();
}

// ─── Connection item context menu ─────────────────────────────────────────────

test.describe("ContextMenu — Connection item", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await clearConnections(mockedPage);
    await mockedPage.reload();
    await goToMain(mockedPage);
    await expect(mockedPage.locator(".conn-item")).toBeVisible();
  });

  test("right-clicking a connection item opens the context menu", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await expect(mockedPage.locator(".ctx-menu")).toBeVisible();
  });

  test("context menu title shows the connection name", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await expect(mockedPage.locator(".ctx-menu-title")).toHaveText("Test VNG Server");
  });

  test("context menu shows Connect/Reconnect item", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    const connectItem = mockedPage.locator(".ctx-menu-item .ctx-menu-label").filter({
      hasText: /^(Connect|Reconnect)$/,
    });
    await expect(connectItem).toBeVisible();
  });

  test("context menu shows Disconnect item", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await expect(
      mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "Disconnect" })
    ).toBeVisible();
  });

  test("context menu shows Refresh Schema item", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await expect(
      mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "Refresh Schema" })
    ).toBeVisible();
  });

  test("context menu shows Edit Connection item", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await expect(
      mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "Edit Connection…" })
    ).toBeVisible();
  });

  test("context menu shows Duplicate item", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await expect(
      mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "Duplicate" })
    ).toBeVisible();
  });

  test("context menu shows New Database item", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await expect(
      mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "New Database…" })
    ).toBeVisible();
  });

  test("context menu shows New User item", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await expect(
      mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "New User…" })
    ).toBeVisible();
  });

  test("context menu shows Remove Connection item with danger styling", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    const removeItem = mockedPage.locator(".ctx-menu-item.danger .ctx-menu-label", {
      hasText: "Remove Connection",
    });
    await expect(removeItem).toBeVisible();
  });

  test("context menu shows separators between groups", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await expect(mockedPage.locator(".ctx-menu-sep").first()).toBeVisible();
  });

  // ── Dismissal ────────────────────────────────────────────────────────────

  test("pressing Escape closes the context menu", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await expect(mockedPage.locator(".ctx-menu")).toBeVisible();
    await mockedPage.keyboard.press("Escape");
    await expect(mockedPage.locator(".ctx-menu")).not.toBeVisible();
  });

  test("clicking outside the context menu closes it", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await expect(mockedPage.locator(".ctx-menu")).toBeVisible();
    // Click outside the menu (the editor area or empty space)
    await mockedPage.locator(".editor-area").click({ force: true });
    await expect(mockedPage.locator(".ctx-menu")).not.toBeVisible();
  });

  test("only one context menu is open at a time", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await expect(mockedPage.locator(".ctx-menu")).toHaveCount(1);
  });

  // ── Menu item actions ─────────────────────────────────────────────────────

  test("clicking Edit Connection opens connection panel", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "Edit Connection…" }).click();
    await expect(mockedPage.locator(".conn-panel")).toBeVisible();
    // Context menu should close after selecting an item
    await expect(mockedPage.locator(".ctx-menu")).not.toBeVisible();
  });

  test("clicking New Database opens create-database modal", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "New Database…" }).click();
    await expect(mockedPage.locator(".conn-panel-title")).toContainText("Create Database");
    await expect(mockedPage.locator(".ctx-menu")).not.toBeVisible();
  });

  test("clicking New User opens create-user modal", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "New User…" }).click();
    await expect(mockedPage.locator(".conn-panel-title")).toContainText("Create User");
    await expect(mockedPage.locator(".ctx-menu")).not.toBeVisible();
  });
});

// ─── Schema tree context menus ────────────────────────────────────────────────

test.describe("ContextMenu — Schema tree", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await clearConnections(mockedPage);
    await mockedPage.reload();
    await goToMain(mockedPage);
    // Wait for schema tree to render (mocked API returns immediately)
    await mockedPage.waitForTimeout(400);
  });

  test("right-clicking a database node shows database context menu", async ({ mockedPage }) => {
    const dbNode = mockedPage.locator(".tree-node").filter({ hasText: "default" }).first();
    if (await dbNode.isVisible()) {
      await dbNode.click({ button: "right" });
      await expect(mockedPage.locator(".ctx-menu")).toBeVisible();
      await expect(
        mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "New Schema…" })
      ).toBeVisible();
      await expect(
        mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "View DDL" })
      ).toBeVisible();
      await expect(
        mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "Drop Database…" })
      ).toBeVisible();
      await mockedPage.keyboard.press("Escape");
    }
  });

  test("right-clicking a table node shows table context menu", async ({ mockedPage }) => {
    const tableNode = mockedPage.locator(".tree-node .tree-icon", { hasText: "📋" }).first();
    if (await tableNode.isVisible()) {
      await tableNode.locator("..").click({ button: "right" });
      await expect(mockedPage.locator(".ctx-menu")).toBeVisible();
      await expect(
        mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "View DDL" })
      ).toBeVisible();
      await mockedPage.keyboard.press("Escape");
    }
  });
});

// ─── Users panel context menu ─────────────────────────────────────────────────

test.describe("ContextMenu — Users panel", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await clearConnections(mockedPage);
    await mockedPage.reload();
    await goToMain(mockedPage);
    await mockedPage.locator(".activity-btn", { hasText: "Users" }).click();
    await expect(mockedPage.locator(".conn-item").first()).toBeVisible();
  });

  test("right-clicking a user item shows user context menu", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await expect(mockedPage.locator(".ctx-menu")).toBeVisible();
  });

  test("user context menu shows Edit User item", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await expect(
      mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "Edit User…" })
    ).toBeVisible();
  });

  test("user context menu shows Grant Role item", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await expect(
      mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "Grant Role…" })
    ).toBeVisible();
  });

  test("user context menu shows Drop User item with danger styling", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    const dropItem = mockedPage.locator(".ctx-menu-item.danger .ctx-menu-label", {
      hasText: "Drop User…",
    });
    await expect(dropItem).toBeVisible();
    await mockedPage.keyboard.press("Escape");
  });

  test("clicking Grant Role from user context menu opens grant-role modal", async ({ mockedPage }) => {
    await mockedPage.locator(".conn-item").first().click({ button: "right" });
    await mockedPage.locator(".ctx-menu-item .ctx-menu-label", { hasText: "Grant Role…" }).click();
    await expect(mockedPage.locator(".conn-panel-title")).toContainText("Grant Role");
  });
});
