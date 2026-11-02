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

test.describe('reduced motion switched on during the visit', () => {
  test.use({ contextOptions: { reducedMotion: 'no-preference' } });
  test('motion stops at once: no loops, the typing ends whole, "See it answer" jumps', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 700 });
    await page.goto('/');
    await expect(page.locator('html')).toHaveClass(/motion-ok/);
    // start the terminal typing (it takes ~1.35s), then switch the setting mid-way
    await page.evaluate(() => window.scrollTo({ top: document.getElementById('get-started')!.getBoundingClientRect().top + scrollY - 64, behavior: 'instant' }));
    await page.waitForTimeout(350);
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await expect(page.locator('html')).not.toHaveClass(/motion-ok/);
    const gs = page.locator('#get-started');
    await expect.poll(() => gs.evaluate((el) => [...el.querySelectorAll('.gs-step .cmd-type')].map((t) => t.textContent).join('|')))
      .toBe(await gs.evaluate((el) => [...el.querySelectorAll('.gs-step .cmd-full')].map((t) => t.textContent).join('|')));
    await expect.poll(() => page.evaluate(() => document.getAnimations().filter((a) => a.playState === 'running' && a.effect?.getComputedTiming().iterations === Infinity).length)).toBe(0);
    // "See it answer" asks for an instant jump, not a smooth scroll
    const behaviors = await page.evaluate(async () => {
      const seen: string[] = [];
      const orig = window.scrollTo.bind(window);
      window.scrollTo = ((a?: ScrollToOptions | number, b?: number) => {
        if (typeof a === 'object' && a) seen.push(String(a.behavior));
        return typeof a === 'number' ? orig(a, b ?? 0) : orig(a);
      }) as typeof window.scrollTo;
      const t = document.getElementById('toolface')!;
      window.scrollTo({ top: t.getBoundingClientRect().top + scrollY - 64, behavior: 'instant' });
      seen.length = 0;
      (t.querySelector('.see-btn') as HTMLElement).click();
      await new Promise((r) => setTimeout(r, 120));
      return seen;
    });
    expect(behaviors).toEqual(['auto']);
  });
});
