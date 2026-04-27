/**
 * theme.spec.ts
 *
 * End-to-end tests for the TitleBar theme toggle system.
 * Covers: theme menu open/close, light/dark/system selection,
 * data-theme DOM attribute propagation, and localStorage persistence.
 */

import { test, expect, seedConnection, clearConnections } from "./helpers/fixtures";

const THEME_STORE_KEY = "vng-studio-theme";

// ─── Helper ───────────────────────────────────────────────────────────────────

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

// ─── Theme Menu ───────────────────────────────────────────────────────────────

test.describe("Theme Toggle", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await clearConnections(mockedPage);
    await mockedPage.evaluate((key) => localStorage.removeItem(key), THEME_STORE_KEY);
    await mockedPage.reload();
    await goToMain(mockedPage);
  });

  // ── Visibility ─────────────────────────────────────────────────────────────

  test("theme button is visible in TitleBar", async ({ mockedPage }) => {
    // The theme button shows the current theme icon (☀, ☾, or ◐)
    await expect(mockedPage.locator(".theme-menu-anchor")).toBeVisible();
    const themeBtn = mockedPage.locator(".theme-menu-anchor .titlebar-btn");
    await expect(themeBtn).toBeVisible();
  });

  test("theme button shows a theme icon (☀, ☾, or ◐)", async ({ mockedPage }) => {
    const themeBtn = mockedPage.locator(".theme-menu-anchor .titlebar-btn");
    const text = await themeBtn.textContent();
    expect(["☀", "☾", "◐"]).toContain(text?.trim());
  });

  // ── Dropdown open/close ────────────────────────────────────────────────────

  test("clicking theme button opens theme dropdown menu", async ({ mockedPage }) => {
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    await expect(mockedPage.locator(".theme-menu")).toBeVisible();
  });

  test("theme dropdown shows Light, Dark, and System options", async ({ mockedPage }) => {
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    const menu = mockedPage.locator(".theme-menu");
    await expect(menu.locator("button", { hasText: "Light" })).toBeVisible();
    await expect(menu.locator("button", { hasText: "Dark" })).toBeVisible();
    await expect(menu.locator("button", { hasText: "System" })).toBeVisible();
  });

  test("theme button has 'active' class while menu is open", async ({ mockedPage }) => {
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    await expect(mockedPage.locator(".theme-menu-anchor .titlebar-btn.active")).toBeVisible();
  });

  test("clicking theme button a second time closes the menu", async ({ mockedPage }) => {
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    await expect(mockedPage.locator(".theme-menu")).toBeVisible();
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    await expect(mockedPage.locator(".theme-menu")).not.toBeVisible();
  });

  test("clicking outside the theme menu closes it", async ({ mockedPage }) => {
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    await expect(mockedPage.locator(".theme-menu")).toBeVisible();
    // Click somewhere outside the theme menu anchor
    await mockedPage.locator(".titlebar-logo").click();
    await expect(mockedPage.locator(".theme-menu")).not.toBeVisible();
  });

  // ── Theme Selection ────────────────────────────────────────────────────────

  test("selecting Light sets data-theme='light' on html element", async ({ mockedPage }) => {
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    await mockedPage.locator(".theme-menu button", { hasText: "Light" }).click();
    const dataTheme = await mockedPage.evaluate(() =>
      document.documentElement.getAttribute("data-theme")
    );
    expect(dataTheme).toBe("light");
  });

  test("selecting Dark sets data-theme='dark' on html element", async ({ mockedPage }) => {
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    await mockedPage.locator(".theme-menu button", { hasText: "Dark" }).click();
    const dataTheme = await mockedPage.evaluate(() =>
      document.documentElement.getAttribute("data-theme")
    );
    expect(dataTheme).toBe("dark");
  });

  test("selecting a theme closes the dropdown", async ({ mockedPage }) => {
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    await mockedPage.locator(".theme-menu button", { hasText: "Dark" }).click();
    await expect(mockedPage.locator(".theme-menu")).not.toBeVisible();
  });

  test("selected theme has 'active' class in dropdown", async ({ mockedPage }) => {
    // Set to light first
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    await mockedPage.locator(".theme-menu button", { hasText: "Light" }).click();
    // Reopen menu
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    const lightBtn = mockedPage.locator(".theme-menu button", { hasText: "Light" });
    await expect(lightBtn).toHaveClass(/active/);
  });

  test("theme button icon changes after selecting Light", async ({ mockedPage }) => {
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    await mockedPage.locator(".theme-menu button", { hasText: "Light" }).click();
    const themeBtn = mockedPage.locator(".theme-menu-anchor .titlebar-btn");
    await expect(themeBtn).toContainText("☀");
  });

  test("theme button icon changes after selecting Dark", async ({ mockedPage }) => {
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    await mockedPage.locator(".theme-menu button", { hasText: "Dark" }).click();
    const themeBtn = mockedPage.locator(".theme-menu-anchor .titlebar-btn");
    await expect(themeBtn).toContainText("☾");
  });

  test("theme button icon shows ◐ for System mode", async ({ mockedPage }) => {
    // Set to dark first, then switch to system
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    await mockedPage.locator(".theme-menu button", { hasText: "Dark" }).click();
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    await mockedPage.locator(".theme-menu button", { hasText: "System" }).click();
    const themeBtn = mockedPage.locator(".theme-menu-anchor .titlebar-btn");
    await expect(themeBtn).toContainText("◐");
  });

  // ── Persistence ────────────────────────────────────────────────────────────

  test("selected theme mode is persisted to localStorage", async ({ mockedPage }) => {
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    await mockedPage.locator(".theme-menu button", { hasText: "Light" }).click();
    const stored = await mockedPage.evaluate((key: string) => {
      const raw = localStorage.getItem(key);
      if (!raw) return null;
      return JSON.parse(raw) as { state: { mode: string } };
    }, THEME_STORE_KEY);
    expect(stored?.state?.mode).toBe("light");
  });

  test("theme persists across page reload", async ({ mockedPage }) => {
    await mockedPage.locator(".theme-menu-anchor .titlebar-btn").click();
    await mockedPage.locator(".theme-menu button", { hasText: "Dark" }).click();
    await mockedPage.reload();
    await goToMain(mockedPage);
    const dataTheme = await mockedPage.evaluate(() =>
      document.documentElement.getAttribute("data-theme")
    );
    expect(dataTheme).toBe("dark");
  });

  // ── Welcome screen ─────────────────────────────────────────────────────────

  test("theme toggle is also available on welcome screen", async ({ mockedPage }) => {
    // Navigate back to welcome
    await clearConnections(mockedPage);
    await mockedPage.reload();
    await expect(mockedPage.locator(".theme-menu-anchor")).toBeVisible();
  });
});
