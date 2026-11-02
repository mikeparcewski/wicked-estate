import { test, expect } from '@playwright/test';
import { clickUntil } from './helpers';

// GetStarted (#get-started) — every command row carries a copy-to-clipboard
// button (the one thing this site was missing vs its siblings). Clicking one
// flips it to the "copied" state and puts the exact command on the clipboard.

test.use({ permissions: ['clipboard-read', 'clipboard-write'] });

test('copy buttons copy the exact command and show the copied state', async ({ page }) => {
  await page.goto('/');
  const section = page.locator('#get-started');
  await section.scrollIntoViewIfNeeded();

  // one per command: the installer + the three direct-install commands
  await expect(section.locator('.copy-btn')).toHaveCount(4);

  // the primary installer command
  const installerBtn = section.locator('.copy-btn').first();
  const w0 = (await installerBtn.boundingBox())?.width;
  await clickUntil(installerBtn, async () => (await installerBtn.getAttribute('data-copied')) === 'true');
  await expect(installerBtn).toHaveText(/copied/, { useInnerText: true });
  // the label swap never resizes the button (it would squeeze the command beside it)
  expect((await installerBtn.boundingBox())?.width).toBe(w0);
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('npx wicked-installer');

  // a multi-line display command still copies as ONE shell-ready line
  const mcpBtn = section.locator('.copy-btn').last();
  await clickUntil(mcpBtn, async () => (await mcpBtn.getAttribute('data-copied')) === 'true');
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
    'claude mcp add wicked-estate -s project -- wicked-estate-mcp --db "$PWD/graph.db"',
  );

  // copying a step marks its number
  await expect(section.locator('.gs-step').last()).toHaveAttribute('data-done', 'true');

  // the copied state resets so the button is reusable
  await expect(mcpBtn).toHaveText('copy', { timeout: 5_000, useInnerText: true });
});

test('a command never breaks inside a token on a phone', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 740 });
  await page.goto('/');
  const section = page.locator('#get-started');
  await section.scrollIntoViewIfNeeded();
  await expect(section.locator('.gs-step')).toHaveCount(3);
  // every token (a word, a flag such as -s) sits on one line
  const split = await section.evaluate((el) =>
    [...el.querySelectorAll('.cmd-full .tok')].filter((t) => t.getClientRects().length > 1).map((t) => t.textContent),
  );
  expect(split).toEqual([]);
});
