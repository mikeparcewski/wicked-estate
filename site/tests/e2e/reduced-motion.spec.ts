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

// wicked-web#31: global.css re-declares `scroll-snap-type` on html, so it carries the chrome's guard.
test('reduced motion: no scroll snapping at desktop width', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 700 });
  await page.goto('/');
  expect(await page.evaluate(() => matchMedia('(prefers-reduced-motion: reduce)').matches)).toBe(true);
  expect(await page.evaluate(() => getComputedStyle(document.documentElement).scrollSnapType)).toBe('none');
});
