import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  timeout: 30000,
  fullyParallel: true,
  use: {
    baseURL: "http://127.0.0.1:8787",
    trace: "retain-on-failure",
    channel: process.env.PLAYWRIGHT_CHANNEL || undefined,
  },
  webServer: {
    command: "node scripts/web-test-server.mjs",
    cwd: process.cwd(),
    url: "http://127.0.0.1:8787",
    reuseExistingServer: !process.env.CI,
  },
  reporter: "list",
});
