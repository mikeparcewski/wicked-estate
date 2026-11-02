import { test, expect, devices } from '@playwright/test';
import { clickUntil } from './helpers';

// Mobile (iPhone 12, 390x844): a deliberate phone layout. The five questions wrap (no hidden
// side-scroller), the demo stays labelled as scripted, picking a question never moves the page,
// the explorer still searches, tap targets are at least 44px, nothing scrolls sideways.

const { defaultBrowserType: _ignored, ...iphone12 } = devices['iPhone 12'];
test.use(iphone12);

test('mobile: the playground and the tool explorer work, no horizontal overflow', async ({ page }) => {
  const pageErrors: Error[] = [];
  page.on('pageerror', (e) => pageErrors.push(e));
  await page.goto('/');
  await expect(page.locator('h1')).toBeVisible();
  await expect(page.locator('#menuBtn')).toBeVisible();

  const q = page.locator('#query');
  await expect(q.locator('.q-scripted')).toBeVisible();
  // all five questions are fully on screen horizontally: wrapped, not a side-scroller
  for (let i = 0; i < 5; i++) {
    const b = await q.locator('.q-ask').nth(i).boundingBox();
    expect(b && b.x >= 0 && b.x + b.width <= 390).toBe(true);
    expect(b && b.height >= 44).toBe(true);
  }
  const next = () => page.locator('#stores').evaluate((el) => Math.round(el.getBoundingClientRect().top + scrollY));
  const n0 = await next();
  const stack = q.getByRole('option', { name: 'Can two coupons stack?' });
  await clickUntil(stack, async () => (await q.getAttribute('data-q')) === 'stack');
  await expect(q.locator('.q-card').first()).toContainText('docs/pricing.md');
  await q.getByRole('option', { name: 'Everything about applyDiscount' }).click();
  await q.getByRole('option', { name: 'Who reacts when an order is paid?' }).click();
  expect(await next(), 'picking a question moved the page').toBe(n0);
  for (const sel of ['.q-tab', '.q-stop']) {
    const b = await q.locator(sel).first().boundingBox();
    expect(b && b.height >= 40, `${sel} tap target`).toBe(true);
  }

  const t = page.locator('#toolface');
  await t.scrollIntoViewIfNeeded();
  await t.getByLabel('Search tools').fill('blast');
  await expect(t.locator('.tl-row')).toContainText('BlastRadius');

  for (const id of ['#stores', '#provenance', '#storage', '#foundation', '#get-started']) {
    await page.locator(id).scrollIntoViewIfNeeded();
    await expect(page.locator(id)).toBeVisible();
  }
  const copy = await page.locator('#get-started .copy-btn').first().boundingBox();
  expect(copy && copy.height >= 44).toBe(true);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(1);
  expect(pageErrors).toEqual([]);
});
