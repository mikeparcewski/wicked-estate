import { test, expect } from '@playwright/test';

// prefers-reduced-motion: every auto-animation on the page is gated behind
// useReducedMotion(); the IDE swaps to a static, representative result set.

test.use({ contextOptions: { reducedMotion: 'reduce' } });

test('reduced motion: page loads with zero page errors and key sections visible', async ({ page }) => {
  const pageErrors: Error[] = [];
  page.on('pageerror', (e) => pageErrors.push(e));

  await page.goto('/');

  for (const id of ['#query', '#strata', '#toolface', '#binary', '#provenance', '#storage', '#foundation', '#get-started']) {
    await page.locator(id).scrollIntoViewIfNeeded();
    await expect(page.locator(id)).toBeVisible();
  }

  // the IDE renders its static reduced-motion state: no cursor/menu overlays,
  // the all-details peek pre-landed in the dock
  await expect(page.locator('#query .ide-dock-title')).toHaveText('PricingService — all details');
  await expect(page.locator('#query .ide-cursor')).toHaveCount(0);
  await expect(page.locator('#query .ide-ctxmenu')).toHaveCount(0);

  expect(pageErrors).toEqual([]);
});

// wicked-web#31: snapping moves the viewport for you, the motion the preference asks the page to
// suppress. global.css re-declares `scroll-snap-type` on html, so it carries the chrome's guard too;
// assert at a desktop width, where snapping would otherwise be on (`y proximity`).
test('reduced motion: no scroll snapping at desktop width', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 700 });
  await page.goto('/');
  expect(await page.evaluate(() => matchMedia('(prefers-reduced-motion: reduce)').matches)).toBe(true);
  expect(await page.evaluate(() => getComputedStyle(document.documentElement).scrollSnapType)).toBe('none');
});
