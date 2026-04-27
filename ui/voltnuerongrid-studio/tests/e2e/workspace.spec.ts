import { test, expect, seedConnection, clearConnections } from "./helpers/fixtures";

async function goToMainWithConnection(page: Parameters<typeof seedConnection>[0]) {
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

test.describe("Workspace & TabBar", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await clearConnections(mockedPage);
    await mockedPage.reload();
    await goToMainWithConnection(mockedPage);
  });

  // ── TabBar structure ───────────────────────────────────────────────────────

  test("TabBar is visible with at least one tab open by default", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".tabbar")).toBeVisible();
    await expect(mockedPage.locator(".tab")).toHaveCount(1);
  });

  test("default tab is a SQL tab named query_1.sql", async ({ mockedPage }) => {
    const tab = mockedPage.locator(".tab").first();
    await expect(tab).toBeVisible();
    await expect(tab.locator(".tab-label")).toContainText("query_1.sql");
    await expect(tab.locator(".tab-icon")).toContainText("📄");
  });

  test("first tab is active by default", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".tab.active")).toHaveCount(1);
    await expect(mockedPage.locator(".tab.active .tab-label")).toContainText("query_1.sql");
  });

  test("+ button opens a new SQL tab", async ({ mockedPage }) => {
    await mockedPage.locator(".tab-new-btn").click();
    await expect(mockedPage.locator(".tab")).toHaveCount(2);
    await expect(mockedPage.locator(".tab.active .tab-label")).toContainText("query_2.sql");
  });

  test("clicking a tab makes it active", async ({ mockedPage }) => {
    await mockedPage.locator(".tab-new-btn").click();
    await mockedPage.locator(".tab").first().click();
    await expect(mockedPage.locator(".tab.active .tab-label")).toContainText("query_1.sql");
  });

  test("close button on tab removes it from the tab bar", async ({ mockedPage }) => {
    await mockedPage.locator(".tab-new-btn").click();
    await expect(mockedPage.locator(".tab")).toHaveCount(2);
    // Close first tab
    await mockedPage.locator(".tab").first().locator(".tab-close").click();
    await expect(mockedPage.locator(".tab")).toHaveCount(1);
  });

  test("close button click does not trigger tab activation", async ({ mockedPage }) => {
    await mockedPage.locator(".tab-new-btn").click();
    const firstTabLabel = await mockedPage.locator(".tab").first().locator(".tab-label").textContent();
    // Make second tab active, then close first without activating it
    await mockedPage.locator(".tab").nth(1).click();
    await mockedPage.locator(".tab").first().locator(".tab-close").click();
    // Remaining tab should still be query_2
    await expect(mockedPage.locator(".tab.active .tab-label")).not.toContainText(firstTabLabel ?? "");
  });

  test("closing extra tabs keeps at least one tab open", async ({ mockedPage }) => {
    // Open an extra tab so we have > 1
    await mockedPage.locator(".tab-new-btn").click();
    const countBefore = await mockedPage.locator(".tab").count();
    expect(countBefore).toBeGreaterThan(1);
    // Close all but the last one
    for (let i = countBefore - 1; i >= 1; i--) {
      await mockedPage.locator(".tab").nth(i).locator(".tab-close").click();
    }
    // App always keeps at least 1 tab (by design)
    await expect(mockedPage.locator(".tab")).toHaveCount(1);
    // Workspace content is still visible
    await expect(mockedPage.locator(".editor-area")).toBeVisible();
  });

  test("dirty tab indicator (●) appears when SQL is edited", async ({ mockedPage }) => {
    const textarea = mockedPage.locator(".monaco-editor textarea").first();
    if (await textarea.isVisible({ timeout: 2000 }).catch(() => false)) {
      await textarea.fill("SELECT 1;");
      await expect(mockedPage.locator(".tab-dirty")).toBeVisible({ timeout: 2000 });
    }
  });

  // ── Workspace content ─────────────────────────────────────────────────────

  test("Toolbar is rendered for active SQL tab", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".toolbar")).toBeVisible();
  });

  test("SQL editor area is rendered", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".editor-area")).toBeVisible();
  });

  test("ResultsPane is rendered below the editor", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".results-pane")).toBeVisible();
  });

  test("workspace always keeps at least one tab open (last tab not closeable)", async ({ mockedPage }) => {
    // Ensure only 1 tab is open by closing extras first
    const count = await mockedPage.locator(".tab").count();
    for (let i = count - 1; i >= 1; i--) {
      await mockedPage.locator(".tab").nth(i).locator(".tab-close").click();
    }
    await expect(mockedPage.locator(".tab")).toHaveCount(1);
    // Try to close the last tab — it should NOT close (app guards this)
    await mockedPage.locator(".tab").first().locator(".tab-close").click();
    await expect(mockedPage.locator(".tab")).toHaveCount(1);
  });

  // ── Tab icon types ─────────────────────────────────────────────────────────

  test("SQL tab shows 📄 icon", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".tab.active .tab-icon")).toContainText("📄");
  });

  test("multiple tabs can be opened and cycled", async ({ mockedPage }) => {
    for (let i = 0; i < 3; i++) {
      await mockedPage.locator(".tab-new-btn").click();
    }
    await expect(mockedPage.locator(".tab")).toHaveCount(4);
    // Cycle through them
    for (let i = 0; i < 4; i++) {
      await mockedPage.locator(".tab").nth(i).click();
      await expect(mockedPage.locator(".tab").nth(i)).toHaveClass(/active/);
    }
  });
});
