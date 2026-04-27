import { test, expect, seedConnection, clearConnections } from "./helpers/fixtures";

test.describe("Welcome Screen", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await clearConnections(mockedPage);
    await mockedPage.reload();
  });

  test("renders the VoltNueronGrid Studio title and logo", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".welcome-logo")).toBeVisible();
    await expect(mockedPage.locator(".welcome-title")).toContainText("VoltNueronGrid");
    await expect(mockedPage.locator(".welcome-title")).toContainText("Studio");
  });

  test("renders the tagline subtitle", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".welcome-sub")).toContainText("HTAP workloads");
  });

  test("shows New Connection card", async ({ mockedPage }) => {
    const card = mockedPage.locator(".welcome-card").filter({ hasText: "New Connection" });
    await expect(card).toBeVisible();
    await expect(card.locator(".wc-icon")).toContainText("⚡");
    await expect(card.locator(".wc-title")).toHaveText("New Connection");
    await expect(card.locator(".wc-desc")).toContainText("Connect to a database");
  });

  test("shows New Query card", async ({ mockedPage }) => {
    const card = mockedPage.locator(".welcome-card").filter({ hasText: "New Query" });
    await expect(card).toBeVisible();
    await expect(card.locator(".wc-title")).toHaveText("New Query");
  });

  test("shows Dashboard card", async ({ mockedPage }) => {
    const card = mockedPage.locator(".welcome-card").filter({ hasText: "Dashboard" });
    await expect(card).toBeVisible();
    await expect(card.locator(".wc-title")).toHaveText("Dashboard");
    await expect(card.locator(".wc-desc")).toContainText("Monitor cluster health");
  });

  test("clicking New Connection card opens the connection panel", async ({ mockedPage }) => {
    await mockedPage.locator(".welcome-card").filter({ hasText: "New Connection" }).click();
    await expect(mockedPage.locator(".conn-panel")).toBeVisible();
    await expect(mockedPage.locator(".conn-panel-title")).toHaveText("New Connection");
  });

  test("clicking New Query card navigates to main workspace", async ({ mockedPage }) => {
    await mockedPage.locator(".welcome-card").filter({ hasText: "New Query" }).click();
    await expect(mockedPage.locator(".workspace")).toBeVisible();
    await expect(mockedPage.locator(".tabbar")).toBeVisible();
  });

  test("clicking Dashboard card navigates to dashboard screen", async ({ mockedPage }) => {
    await mockedPage.locator(".welcome-card").filter({ hasText: "Dashboard" }).click();
    await expect(mockedPage.locator(".dashboard")).toBeVisible();
  });

  test("does not show Recent Connections section when no connections exist", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".recent-list")).not.toBeVisible();
    await expect(mockedPage.locator(".section-label")).not.toBeVisible();
  });

  test("shows Recent Connections section when connections exist", async ({ mockedPage }) => {
    await seedConnection(mockedPage);
    await mockedPage.reload();
    await expect(mockedPage.locator(".recent-list")).toBeVisible();
    await expect(mockedPage.locator(".section-label")).toContainText("Recent Connections");
  });

  test("recent connection entry shows server type badge", async ({ mockedPage }) => {
    await seedConnection(mockedPage);
    await mockedPage.reload();
    const item = mockedPage.locator(".recent-item").first();
    await expect(item).toBeVisible();
    await expect(item.locator(".conn-type-badge")).toBeVisible();
  });

  test("recent connection entry shows host and port", async ({ mockedPage }) => {
    await seedConnection(mockedPage);
    await mockedPage.reload();
    const item = mockedPage.locator(".recent-item").first();
    await expect(item).toContainText("127.0.0.1");
    await expect(item).toContainText("8080");
  });

  test("clicking a recent connection navigates to main screen", async ({ mockedPage }) => {
    await seedConnection(mockedPage);
    await mockedPage.reload();
    await mockedPage.locator(".recent-item").first().click();
    await expect(mockedPage.locator(".workspace")).toBeVisible();
  });

  test("shows at most 5 recent connections", async ({ mockedPage }) => {
    // Seed 6 connections using the correct Zustand persist key
    await mockedPage.evaluate(() => {
      const conns = Array.from({ length: 6 }, (_, i) => ({
        id: `conn-${i}`,
        name: `Connection ${i}`,
        serverType: "voltnuerongrid",
        runtimeTarget: "local",
        baseUrl: `http://127.0.0.1:${8080 + i}`,
        host: "127.0.0.1",
        port: 8080 + i,
        mode: "admin",
        sslEnabled: false,
        createdAt: Date.now() - i * 10_000,
        lastUsed: Date.now() - i * 10_000,
      }));
      const state = { state: { connections: conns, activeId: "conn-0" }, version: 0 };
      localStorage.setItem("vng-studio-connections", JSON.stringify(state));
    });
    await mockedPage.reload();
    const items = mockedPage.locator(".recent-item");
    await expect(items).toHaveCount(5);
  });

  test("TitleBar is always visible on welcome screen", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".titlebar, [class*='titlebar'], [class*='title-bar']").first()).toBeVisible();
  });
});
