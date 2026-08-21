import { defineConfig, devices } from "@playwright/test";

const port = Number.parseInt(process.env.MINIEXCEL_WEB_PORT ?? "4173", 10);

export default defineConfig({
  testDir: "./tests",
  timeout: 30_000,
  expect: { timeout: 10_000 },
  reporter: "line",
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    trace: "retain-on-failure",
  },
  webServer: {
    command: `node scripts/serve.mjs --port ${port}`,
    port,
    reuseExistingServer: true,
  },
  projects: [
    { name: "desktop", use: { ...devices["Desktop Chrome"] } },
    { name: "mobile", use: { ...devices["Pixel 7"] } },
  ],
});
