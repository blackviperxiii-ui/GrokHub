import { defineConfig, devices } from "@playwright/test";

const baseURL = (
  process.env.PLAYWRIGHT_BASE_URL ||
  process.env.GROKHUB_URL ||
  "http://127.0.0.1:18765"
).replace(/\/$/, "");

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  reporter: process.env.CI ? [["list"], ["github"]] : "list",
  use: {
    ...devices["Desktop Chrome"],
    baseURL,
    trace: "off",
    screenshot: "off",
    video: "off",
  },
});
