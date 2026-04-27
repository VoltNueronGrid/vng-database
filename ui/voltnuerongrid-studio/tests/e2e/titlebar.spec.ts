/**
 * titlebar.spec.ts
 *
 * End-to-end tests for the TitleBar component.
 * Covers: traffic light buttons, connection badge, schema refresh,
 * disconnect, new connection, and navigation to dashboard.
 */

import { test, expect, seedConnection, clearConnections } from "./helpers/fixtures";

// ─── Helpers ──────────────────────────────────────────────────────────────────

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

async function goToWelcomeNoConnection(page: Parameters<typeof seedConnection>[0]) {
  await clearConnections(page);
  await page.reload();
  await expect(page.locator(".welcome-title")).toBeVisible();
}

// ─── TitleBar — Always visible ────────────────────────────────────────────────

test.describe("TitleBar — Structure", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await clearConnections(mockedPage);
    await mockedPage.reload();
  });

  test("titlebar is visible on the welcome screen", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".titlebar")).toBeVisible();
  });

  test("logo icon 'V' is visible", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".logo-icon")).toContainText("V");
  });

  test("titlebar shows 'VoltNueronGrid Studio' name", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".titlebar-name")).toContainText("Studio");
  });

  test("traffic-light buttons are visible (macOS placeholder)", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".titlebar-traffic")).toBeVisible();
    await expect(mockedPage.locator(".traffic-close")).toBeVisible();
    await expect(mockedPage.locator(".traffic-min")).toBeVisible();
    await expect(mockedPage.locator(".traffic-max")).toBeVisible();
  });

  test("connection badge is always visible", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".titlebar-conn-badge")).toBeVisible();
  });

  test("badge shows 'No connection' when no connection is active", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".titlebar-conn-badge")).toContainText("No connection");
  });

  test("theme button is always in the titlebar actions", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".theme-menu-anchor")).toBeVisible();
  });

  test("dashboard button is always visible", async ({ mockedPage }) => {
    await expect(
      mockedPage.locator(".titlebar-actions button[title='Dashboard']")
    ).toBeVisible();
  });

  test("New Connection (+) button is always visible", async ({ mockedPage }) => {
    await expect(
      mockedPage.locator(".titlebar-actions button[title='New Connection']")
    ).toBeVisible();
  });
});

// ─── TitleBar — With active connection ────────────────────────────────────────

test.describe("TitleBar — Active connection", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await clearConnections(mockedPage);
    await mockedPage.reload();
    await goToMainWithConnection(mockedPage);
  });

  test("connection badge shows the active connection name", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".titlebar-conn-badge")).toContainText("Test VNG Server");
  });

  test("connection badge shows host and port", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".titlebar-conn-badge")).toContainText("127.0.0.1");
    await expect(mockedPage.locator(".titlebar-conn-badge")).toContainText("8080");
  });

  test("Refresh Schema button is visible when a connection is active", async ({ mockedPage }) => {
    await expect(
      mockedPage.locator(".titlebar-actions button[title='Refresh schema']")
    ).toBeVisible();
  });

  test("Disconnect button is visible when a connection is active", async ({ mockedPage }) => {
    await expect(
      mockedPage.locator(".titlebar-actions button[title='Disconnect']")
    ).toBeVisible();
  });

  test("clicking the connection badge opens the connection panel", async ({ mockedPage }) => {
    await mockedPage.locator(".titlebar-conn-badge").click();
    await expect(mockedPage.locator(".conn-panel")).toBeVisible();
  });

  test("clicking New Connection (+) opens connection panel for a new connection", async ({
    mockedPage,
  }) => {
    await mockedPage.locator(".titlebar-actions button[title='New Connection']").click();
    await expect(mockedPage.locator(".conn-panel")).toBeVisible();
    await expect(mockedPage.locator(".conn-panel-title")).toHaveText("New Connection");
  });

  test("clicking Dashboard button navigates to dashboard screen", async ({ mockedPage }) => {
    await mockedPage.locator(".titlebar-actions button[title='Dashboard']").click();
    await expect(mockedPage.locator(".dashboard")).toBeVisible();
  });

  test("clicking Refresh Schema fires the schema API request", async ({ mockedPage }) => {
    const requests: string[] = [];
    mockedPage.on("request", (req) => {
      if (req.url().includes("/schema/tree")) requests.push(req.url());
    });
    await mockedPage.locator(".titlebar-actions button[title='Refresh schema']").click();
    // Allow debounce / async
    await mockedPage.waitForTimeout(500);
    expect(requests.length).toBeGreaterThan(0);
  });
});

// ─── TitleBar — Disconnect ────────────────────────────────────────────────────

test.describe("TitleBar — Disconnect", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await clearConnections(mockedPage);
    await mockedPage.reload();
    await goToMainWithConnection(mockedPage);
  });

  test("Refresh Schema and Disconnect buttons are NOT shown when no connection is active", async ({
    mockedPage,
  }) => {
    // Start fresh with no connection
    await clearConnections(mockedPage);
    await mockedPage.reload();
    await expect(
      mockedPage.locator(".titlebar-actions button[title='Refresh schema']")
    ).not.toBeVisible();
    await expect(
      mockedPage.locator(".titlebar-actions button[title='Disconnect']")
    ).not.toBeVisible();
  });

  test("clicking Disconnect sets badge to 'No connection'", async ({ mockedPage }) => {
    await mockedPage.locator(".titlebar-actions button[title='Disconnect']").click();
    await expect(mockedPage.locator(".titlebar-conn-badge")).toContainText("No connection");
  });

  test("clicking Disconnect hides the Disconnect button itself", async ({ mockedPage }) => {
    await mockedPage.locator(".titlebar-actions button[title='Disconnect']").click();
    await expect(
      mockedPage.locator(".titlebar-actions button[title='Disconnect']")
    ).not.toBeVisible();
  });

  test("clicking Disconnect hides the Refresh Schema button", async ({ mockedPage }) => {
    await mockedPage.locator(".titlebar-actions button[title='Disconnect']").click();
    await expect(
      mockedPage.locator(".titlebar-actions button[title='Refresh schema']")
    ).not.toBeVisible();
  });

  test("connection badge dot shows 'none' class after disconnect", async ({ mockedPage }) => {
    await mockedPage.locator(".titlebar-actions button[title='Disconnect']").click();
    await expect(mockedPage.locator(".conn-badge-dot.none")).toBeVisible();
  });
});
