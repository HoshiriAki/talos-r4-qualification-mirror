import { defineConfig } from '@playwright/test'
import { fileURLToPath } from 'node:url'

const e2ePort = Number.parseInt(process.env.UI_LAB_E2E_PORT ?? '5199', 10)
const e2eViteConfig = fileURLToPath(new URL('./vite.e2e.config.ts', import.meta.url))
const viteEntrypoint = fileURLToPath(new URL('./node_modules/vite/bin/vite.js', import.meta.url))

// All Playwright outputs (reports, traces, screenshots) land under
// ../.playwright-mcp/ per the project convention.
export default defineConfig({
  testDir: './e2e',
  timeout: 90_000,
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [
    ['list'],
    ['html', { outputFolder: '../.playwright-mcp/report', open: 'never' }],
  ],
  outputDir: '../.playwright-mcp/e2e-results',
  use: {
    baseURL: `http://127.0.0.1:${e2ePort}`,
    trace: 'retain-on-failure',
  },
  webServer: {
    // Keep the config path absolute so the Windows preflight can prove that a
    // stale listener belongs to this worktree before terminating its tree.
    command: `"${process.execPath}" "${viteEntrypoint}" --config "${e2eViteConfig}"`,
    url: `http://127.0.0.1:${e2ePort}/ui-lab`,
    reuseExistingServer: !process.env.CI,
    timeout: 90_000,
    env: {
      VITE_ENABLE_UI_LAB: 'true',
      VITE_BACKEND_TARGET: 'http://127.0.0.1:3000',
    },
  },
  projects: [{ name: 'ui-lab', use: { viewport: { width: 1600, height: 1000 } } }],
})
