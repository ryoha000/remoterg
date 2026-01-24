import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright設定ファイル
 * E2Eテスト用の設定
 */
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: "list",
  use: {
    baseURL: "http://localhost:3000",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
    headless: false,
  },

  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],

  // CI環境では統合スクリプトがサーバーを起動するため、webServerを無効化
  // ...(process.env.CI !== "true" && {
  //   webServer: {
  //     command: "pnpm dev",
  //     url: "http://localhost:3000",
  //     reuseExistingServer: true,
  //     timeout: 120 * 1000,
  //   },
  // }),
});
