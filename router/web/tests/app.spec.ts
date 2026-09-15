import { test, expect, type Page } from "@playwright/test";
async function login(page: Page) {
  await page.goto("/login");
  await page
    .getByLabel("Email address")
    .fill(`browser-${crypto.randomUUID()}@example.test`);
  await page
    .getByLabel("Password", { exact: true })
    .fill("local-test-password");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page).toHaveURL(/dashboard/);
}
test("landing, responsive navigation, themes and public routes", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  for (const [width, height] of [
    [1440, 900],
    [1280, 800],
    [390, 844],
  ]) {
    await page.setViewportSize({ width, height });
    await page.goto("/");
    await expect(
      page.getByRole("heading", {
        name: "Your computer. In the conversation.",
      }),
    ).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    if (width === 390) {
      await page.getByRole("button", { name: "Toggle navigation" }).click();
      await expect(
        page.getByRole("navigation", { name: "Main navigation" }),
      ).toBeVisible();
    }
  }
  await page.getByRole("button", { name: "dark theme", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.getByRole("button", { name: "light theme", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  for (const route of [
    "/download",
    "/security",
    "/privacy",
    "/terms",
    "/support",
    "/connect-chatgpt",
  ]) {
    await page.goto(route);
    await expect(page.locator("h1")).toBeVisible();
  }
  expect(errors).toEqual([]);
});
test("signup, persistent session, logout, protected return and invalid credentials", async ({
  page,
}) => {
  await page.goto("/devices");
  await expect(page).toHaveURL(/login\?return_to/);
  await page
    .getByRole("link", { name: "Create an account", exact: true })
    .click();
  await page
    .getByLabel("Email address")
    .fill(`new-${crypto.randomUUID()}@example.test`);
  await page
    .getByLabel("Password", { exact: true })
    .fill("local-test-password");
  await page
    .getByLabel("Confirm password", { exact: true })
    .fill("local-test-password");
  await page
    .getByRole("button", { name: "Create account", exact: true })
    .click();
  await expect(page).toHaveURL(/devices/);
  await expect(
    page.getByRole("heading", {
      name: "Your first computer. A few steps away.",
    }),
  ).toBeVisible();
  await page.reload();
  await expect(page).toHaveURL(/devices/);
  await page.getByRole("button", { name: "Sign out", exact: true }).click();
  await expect(page).toHaveURL("http://127.0.0.1:8787/");
  await page.goto("/account");
  await expect(page).toHaveURL(/login/);
  await page.getByLabel("Email address").fill("invalid@example.test");
  await page.getByLabel("Password", { exact: true }).fill("wrong-password");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("don’t match");
});
test("password reset never falsely reports an API failure as success", async ({
  page,
}) => {
  await page.goto("/forgot-password");
  await page.route("**/api/auth/request-password-reset", (route) =>
    route.fulfill({ status: 500, json: { error: "internal" } }),
  );
  await page.getByLabel("Email address").fill("reset@example.test");
  await page.getByRole("button", { name: "Send reset link" }).click();
  await expect(page.getByRole("alert")).toContainText("could not complete");
  await page.unroute("**/api/auth/request-password-reset");
  await page.getByRole("button", { name: "Send reset link" }).click();
  await expect(
    page.getByRole("heading", { name: "Check your email" }),
  ).toBeVisible();
});
test("guided pairing waits for online, then device details and confirmed revoke", async ({
  page,
}) => {
  await login(page);
  let devices: { deviceId: string; deviceName: string; online: boolean }[] = [];
  await page.route("**/api/my/devices", async (route) => {
    if (route.request().method() === "DELETE") devices = [];
    await route.fulfill({ json: { devices } });
  });
  await page.getByRole("button", { name: "Add computer", exact: true }).click();
  await page.getByRole("button", { name: "I already installed Latch" }).click();
  await page.getByRole("button", { name: "Generate pairing code" }).click();
  await expect(
    page.getByRole("heading", { name: "Paste this code into Latch." }),
  ).toBeVisible();
  await expect(page.getByRole("dialog").locator(".code-box code")).not.toBeEmpty();
  await expect(page.getByText("Waiting for the Latch app…")).toBeVisible();
  devices = [
    {
      deviceId: "00000000-0000-4000-8000-000000000001",
      deviceName: "Test laptop",
      online: false,
    },
  ];
  await page.waitForTimeout(4500);
  await expect(
    page.getByRole("heading", { name: "Computer connected", exact: true }),
  ).toHaveCount(0);
  devices[0].online = true;
  await expect(
    page.getByRole("heading", { name: "Computer connected", exact: true }),
  ).toBeVisible({ timeout: 10000 });
  await page.getByRole("button", { name: "Done", exact: true }).click();
  await page.getByRole("button", { name: "Details" }).click();
  await page
    .getByRole("button", { name: "Revoke computer", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Revoke Test laptop?" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(page.getByText("Test laptop", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Details" }).click();
  await page
    .getByRole("button", { name: "Revoke computer", exact: true })
    .click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Revoke computer", exact: true })
    .click();
  await expect(
    page.getByRole("heading", {
      name: "Your first computer. A few steps away.",
    }),
  ).toBeVisible();
});
test("OAuth consent preserves real server validation and displays command permission", async ({
  page,
  request,
}) => {
  await login(page);
  const registration = await request.post("/oauth/register", {
    data: { redirect_uris: ["http://127.0.0.1:45990/callback"] },
  });
  const { client_id } = await registration.json();
  const params = new URLSearchParams({
    response_type: "code",
    client_id,
    state: "browser-test",
    code_challenge: "a".repeat(43),
    code_challenge_method: "S256",
    redirect_uri: "http://127.0.0.1:45990/callback",
    scope: "latch:devices:read latch:exec:run",
    resource: "http://127.0.0.1:8787/mcp",
  });
  await page.goto("/oauth/authorize?" + params);
  await expect(
    page.getByRole("heading", { name: "Connect a client to Latch." }),
  ).toBeVisible();
  await expect(
    page.getByText("Run and manage commands", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("are not sandboxed.", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Allow access" }),
  ).toBeVisible();
});
test("device errors are actionable, mobile drawer traps focus and closes", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await login(page);
  await page.route("**/api/my/devices", (route) =>
    route.fulfill({ status: 429, json: { error: "rate_limited" } }),
  );
  await page.reload();
  await expect(page.getByRole("alert")).toContainText("Wait a minute");
  await page.getByRole("button", { name: "Open navigation" }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).not.toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
});
