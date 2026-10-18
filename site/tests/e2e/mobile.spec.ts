import { test, expect, devices } from '@playwright/test';
import { clickUntil } from './helpers';

// Mobile (iPhone 12, 390x844): the playground stacks (questions become a scrolling row) and
// still answers; the tool explorer still searches; nothing scrolls sideways.

const { defaultBrowserType: _ignored, ...iphone12 } = devices['iPhone 12'];
test.use(iphone12);

test('mobile: the playground and the tool explorer work, no horizontal overflow', async ({ page }) => {
  const pageErrors: Error[] = [];
  page.on('pageerror', (e) => pageErrors.push(e));
  await page.goto('/');
  await expect(page.locator('h1')).toBeVisible();
  await expect(page.locator('#menuBtn')).toBeVisible();

  const q = page.locator('#query');
  const stack = q.getByRole('option', { name: 'Can two coupons stack?' });
  await clickUntil(stack, async () => (await q.getAttribute('data-q')) === 'stack');
  await expect(q.locator('.q-card').first()).toContainText('docs/pricing.md');

  const t = page.locator('#toolface');
  await t.scrollIntoViewIfNeeded();
  await t.getByLabel('Search tools').fill('blast');
  await expect(t.locator('.tools-row')).toContainText('BlastRadius');

  for (const id of ['#stores', '#provenance', '#storage', '#foundation', '#get-started']) {
    await page.locator(id).scrollIntoViewIfNeeded();
    await expect(page.locator(id)).toBeVisible();
  }
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(1);
  expect(pageErrors).toEqual([]);
});
