import { test, expect } from '@playwright/test';

test.use({ contextOptions: { reducedMotion: 'reduce' } });

test('reduced motion: zero page errors, every section visible, the playground answers', async ({ page }) => {
  const pageErrors: Error[] = [];
  page.on('pageerror', (e) => pageErrors.push(e));
  await page.goto('/');
  for (const id of ['#query', '#stores', '#provenance', '#toolface', '#storage', '#foundation', '#get-started']) {
    await page.locator(id).scrollIntoViewIfNeeded();
    await expect(page.locator(id)).toBeVisible();
  }
  await expect(page.locator('#query .q-row')).toHaveCount(3);
  expect(pageErrors).toEqual([]);
});

test('reduced motion: every scene shows its end state, nothing plays', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('html')).not.toHaveClass(/motion-ok/);
  const pv = page.locator('#provenance');
  await pv.scrollIntoViewIfNeeded();
  await expect(pv.locator('.pv-panel').first()).toHaveAttribute('data-mode', 'estate');
  const gs = page.locator('#get-started');
  await gs.scrollIntoViewIfNeeded();
  // the terminal shows every command whole (no typing)
  const typed = await gs.evaluate((el) => [...el.querySelectorAll('.gs-step .cmd-type')].map((t) => t.textContent));
  const full = await gs.evaluate((el) => [...el.querySelectorAll('.gs-step .cmd-full')].map((t) => t.textContent));
  expect(typed).toEqual(full);
  expect(full.map((t) => (t ?? '').replace(/\\/g, ' ').replace(/\s+/g, ' ').trim())).toEqual([
    'cargo install wicked-estate wicked-estate-mcp',
    'wicked-estate index . --db graph.db',
    'claude mcp add wicked-estate -s project -- wicked-estate-mcp --db "$PWD/graph.db"',
  ]);
  const running = await page.evaluate(() => document.getAnimations().filter((a) => a.playState === 'running').length);
  expect(running).toBe(0);
});

// wicked-web#31: global.css re-declares `scroll-snap-type` on html, so it carries the chrome's guard.
test('reduced motion: no scroll snapping at desktop width', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 700 });
  await page.goto('/');
  expect(await page.evaluate(() => matchMedia('(prefers-reduced-motion: reduce)').matches)).toBe(true);
  expect(await page.evaluate(() => getComputedStyle(document.documentElement).scrollSnapType)).toBe('none');
});
