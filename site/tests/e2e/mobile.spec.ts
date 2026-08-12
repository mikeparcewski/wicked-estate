import { test, expect, devices } from '@playwright/test';
import { clickUntil } from './helpers';

// Mobile (iPhone 12, 390x844). Content.tsx documents an explicit mobile
// fallback for the IDE widget (≤760px): the desktop split-pane is replaced by
// a static, tappable stacked treatment (.ide-window--mobile) — no auto-play,
// no overlays, nothing that can overflow.

const { defaultBrowserType: _ignored, ...iphone12 } = devices['iPhone 12'];
test.use(iphone12);

test('mobile renders the stacked IDE fallback and no horizontal overflow', async ({ page }) => {
  const pageErrors: Error[] = [];
  page.on('pageerror', (e) => pageErrors.push(e));

  await page.goto('/');
  await expect(page.locator('h1')).toBeVisible();

  // hamburger nav replaces the inline icon cluster at ≤640px
  await expect(page.locator('#menuBtn')).toBeVisible();

  // the documented IDE mobile fallback (post-hydration useIsMobile swap)
  await page.locator('#query').scrollIntoViewIfNeeded();
  await expect(page.locator('#query .ide-window--mobile')).toBeVisible();
  await expect(page.locator('#query .ide-explorer')).toHaveCount(0); // desktop pane gone
  await expect(page.locator('#query .ide-m-cmd')).toHaveCount(8);   // all gestures as tap targets

  // tapping a gesture still lands its result in the dock (Memory tab)
  const recall = page.locator('#query .ide-m-cmd').filter({ hasText: 'Recall decisions' });
  await clickUntil(
    recall,
    async () => (await page.locator('#query #ide-tab-memory').getAttribute('aria-selected')) === 'true',
  );
  await expect(page.locator('#query .ide-dock-title')).toHaveText('Recalled memory');

  // other signature sections still render content on a phone
  for (const id of ['#strata', '#storage', '#foundation', '#get-started']) {
    await page.locator(id).scrollIntoViewIfNeeded();
    await expect(page.locator(id)).toBeVisible();
  }

  // broken-layout tripwire: the page body must never scroll horizontally
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(1);

  expect(pageErrors).toEqual([]);
});
