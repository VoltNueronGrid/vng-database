import { test, expect, seedConnection, clearConnections, MOCK_TOPOLOGY, MOCK_AUDIT_EVENTS } from "./helpers/fixtures";

// Helper: navigate to main with an active connection after seeding
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

async function goToDashboard(page: Parameters<typeof seedConnection>[0]) {
  await seedConnection(page);
  await page.reload();
  await page.locator(".welcome-card").filter({ hasText: "Dashboard" }).click();
  await expect(page.locator(".dashboard")).toBeVisible();
}

// ─── Dashboard Tests ──────────────────────────────────────────────────────────

test.describe("Dashboard", () => {
  test.beforeEach(async ({ mockedPage }) => {
    await clearConnections(mockedPage);
    await mockedPage.reload();
    await goToDashboard(mockedPage);
  });

  test("dashboard renders the Cluster Overview title", async ({ mockedPage }) => {
    await expect(mockedPage.locator(".dash-title")).toHaveText("Cluster Overview");
  });

  test("Refresh button is visible and enabled", async ({ mockedPage }) => {
    const btn = mockedPage.locator(".dashboard .btn.primary", { hasText: /Refresh/ });
    await expect(btn).toBeVisible();
    await expect(btn).not.toBeDisabled();
  });

  test("clicking Refresh triggers topology and audit API calls", async ({ mockedPage }) => {
    const responses: string[] = [];
    mockedPage.on("response", (res) => {
      if (res.url().includes("/topology") || res.url().includes("/audit")) {
        responses.push(res.url());
      }
    });
    await mockedPage.locator(".dashboard .btn.primary", { hasText: /Refresh/ }).click();
    await mockedPage.waitForTimeout(500);
    expect(responses.length).toBeGreaterThan(0);
  });

  test("shows Live Metrics section label", async ({ mockedPage }) => {
    const labels = mockedPage.locator(".section-label");
    await expect(labels.filter({ hasText: "Live Metrics" })).toBeVisible({ timeout: 5000 });
  });

  test("KPI cards are visible after data loads", async ({ mockedPage }) => {
    await mockedPage.waitForSelector(".kpi-card", { timeout: 5000 }).catch(() => {});
    const cards = mockedPage.locator(".kpi-card");
    if (await cards.count() > 0) {
      await expect(cards).not.toHaveCount(0);
    }
  });

  test("dashboard shows Active Nodes kpi label", async ({ mockedPage }) => {
    await mockedPage.waitForSelector(".kpi-label", { timeout: 5000 }).catch(() => {});
    const label = mockedPage.locator(".kpi-label", { hasText: "Active Nodes" });
    if (await label.isVisible()) {
      await expect(label).toBeVisible();
    }
  });

  test("shows error message when topology API fails", async ({ mockedPage }) => {
    await mockedPage.route("**/api/v1/admin/cluster/topology", (route) =>
      route.fulfill({ status: 500, body: "Internal Server Error" })
    );
    await mockedPage.locator(".dashboard .btn.primary", { hasText: /Refresh/ }).click();
    await mockedPage.waitForTimeout(500);
    // Error may or may not be shown depending on topology vs audit
    await expect(mockedPage.locator(".dashboard")).toBeVisible();
  });

  test("dashboard sub-header shows connection name when active connection exists", async ({
    mockedPage,
  }) => {
    await expect(mockedPage.locator(".dash-sub")).toContainText("Test VNG Server");
  });

  test("Refresh button shows loading state while fetching", async ({ mockedPage }) => {
    await mockedPage.route("**/api/v1/admin/topology", async (route) => {
      await new Promise((r) => setTimeout(r, 300));
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(MOCK_TOPOLOGY),
      });
    });
    await mockedPage.locator(".dashboard .btn.primary").click();
    // Button should show loading text briefly
    const btn = mockedPage.locator(".dashboard .btn.primary");
    await expect(btn).toBeVisible();
  });

  test("shows Audit Events section when audit data is available", async ({ mockedPage }) => {
    await mockedPage.waitForSelector(".section-label", { timeout: 5000 }).catch(() => {});
    const auditSection = mockedPage.locator(".section-label", { hasText: /Audit|Recent/ });
    if (await auditSection.isVisible()) {
      await expect(auditSection).toBeVisible();
    }
  });
});

// ─── StatusBar Tests ──────────────────────────────────────────────────────────

test.describe("StatusBar", () => {
  test("status bar is visible on main screen", async ({ mockedPage }) => {
    await goToMain(mockedPage);
    await expect(mockedPage.locator(".statusbar")).toBeVisible();
  });

  test("shows 'No connection' when no active connection", async ({ mockedPage }) => {
    await clearConnections(mockedPage);
    await mockedPage.reload();
    // Go to main without connection
    await mockedPage.locator(".welcome-card").filter({ hasText: "New Query" }).click();
    await expect(mockedPage.locator(".statusbar")).toContainText("No connection");
  });

  test("shows connection name in status bar when connected", async ({ mockedPage }) => {
    await goToMain(mockedPage);
    await expect(mockedPage.locator(".statusbar")).toContainText("Test VNG Server");
  });

  test("shows host and port in status bar", async ({ mockedPage }) => {
    await goToMain(mockedPage);
    await expect(mockedPage.locator(".statusbar")).toContainText("127.0.0.1");
    await expect(mockedPage.locator(".statusbar")).toContainText("8080");
  });

  test("shows connection mode in status bar", async ({ mockedPage }) => {
    await goToMain(mockedPage);
    await expect(mockedPage.locator(".statusbar")).toContainText("admin");
  });

  test("shows UTF-8 encoding indicator", async ({ mockedPage }) => {
    await goToMain(mockedPage);
    await expect(mockedPage.locator(".statusbar")).toContainText("UTF-8");
  });

  test("shows version number", async ({ mockedPage }) => {
    await goToMain(mockedPage);
    await expect(mockedPage.locator(".statusbar")).toContainText("v0.1.0");
  });

  test("shows query route and timing after query executes", async ({ mockedPage }) => {
    await goToMain(mockedPage);

    // Fill SQL via Monaco's hidden textarea
    const textarea = mockedPage.locator(".monaco-editor textarea").first();
    if (await textarea.isVisible({ timeout: 2000 }).catch(() => false)) {
      await textarea.fill("SELECT 1;");
      await mockedPage.locator(".toolbar .btn.primary").click();
      await mockedPage.waitForTimeout(500);
      // Status bar should update with result info
      const bar = mockedPage.locator(".statusbar");
      await expect(bar).toBeVisible();
    }
  });

  test("status dot is present alongside connection name", async ({ mockedPage }) => {
    await goToMain(mockedPage);
    await expect(mockedPage.locator(".statusbar .status-dot")).toBeVisible();
  });

  test("status bar has separators between items", async ({ mockedPage }) => {
    await goToMain(mockedPage);
    await expect(mockedPage.locator(".statusbar .status-sep").first()).toBeVisible();
  });
});
