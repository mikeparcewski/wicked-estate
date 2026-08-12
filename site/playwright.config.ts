import { defineConfig, devices } from '@playwright/test';
const rawPort = process.env.E2E_PORT ?? '4332';
const PORT = Number.parseInt(rawPort, 10);
if (!Number.isInteger(PORT) || PORT < 1 || PORT > 65535) {
  throw new Error(`E2E_PORT must be a port number (1-65535), got "${rawPort}"`);
}
export default defineConfig({
  testDir: './tests/e2e',
  timeout: 30_000,
  expect: { timeout: 10_000 },
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : [['list']],
  use: { baseURL: `http://127.0.0.1:${PORT}`, trace: 'on-first-retry' },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: {
    command: `npm run preview -- --port ${PORT}`,
    url: `http://127.0.0.1:${PORT}`,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
    // Astro 7's `astro preview` daemonizes itself when it detects an AI-agent
    // environment (via am-i-vibing), so the foreground process exits and
    // Playwright reports "Process from config.webServer exited early".
    // Setting ASTRO_PREVIEW_BACKGROUND disables that detection and keeps the
    // server in the foreground (see astro/dist/cli/preview/index.js).
    env: { ASTRO_PREVIEW_BACKGROUND: '1' },
  },
});
