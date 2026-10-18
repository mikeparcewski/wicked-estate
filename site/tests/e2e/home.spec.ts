import { test, expect } from '@playwright/test';

// Baseline smoke: the page renders every section with zero JS errors.

test('home page renders the hook, the playground, every section, and the footer', async ({ page }) => {
  const pageErrors: Error[] = [];
  page.on('pageerror', (e) => pageErrors.push(e));

  await page.goto('/');
  await expect(page).toHaveTitle(/wicked-estate/);

  // The hook leads with memory and knowledge, and the playground sits in the hero.
  await expect(page.locator('h1')).toHaveText('Memory and knowledge for your coding agents.');
  await expect(page.locator('#query')).toBeVisible();

  for (const id of ['#stores', '#provenance', '#toolface', '#storage', '#foundation', '#get-started']) {
    await expect(page.locator(id)).toBeVisible();
  }

  // #foundation is the shared SameGarden map with estate as "you are here".
  const map = page.locator('#foundation .same-garden');
  await expect(map).toBeVisible();
  const here = map.locator('.sg-card--here');
  await expect(here).toHaveCount(1);
  await expect(here).toContainText('wicked-estate');
  await expect(here.locator('.sg-here-chip')).toHaveText('you are here');

  await expect(page.locator('.topbar .wordmark')).toBeVisible();
  await expect(page.locator('footer.footer')).toBeVisible();

  // No stale release callouts: the version comes from Cargo.toml at build time only.
  await expect(page.locator('body')).not.toContainText('v0.15.0');

  expect(pageErrors).toEqual([]);
});

test('the hero, playground included, fits a 1440x700 laptop window', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 700 });
  await page.goto('/');
  await page.evaluate(async () => { await document.fonts.ready; });
  const bottom = await page.locator('#query').evaluate((el) => el.getBoundingClientRect().bottom);
  expect(Math.round(bottom), 'the playground runs below the fold').toBeLessThanOrEqual(700);
  await expect(page.locator('.q-ask').first()).toBeInViewport({ ratio: 1 });
  await expect(page.locator('.q-tab').first()).toBeInViewport({ ratio: 1 });
});

test('the page never scrolls sideways', async ({ page }) => {
  for (const vp of [{ width: 1440, height: 900 }, { width: 390, height: 844 }]) {
    await page.setViewportSize(vp);
    await page.goto('/');
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    expect(overflow, `horizontal overflow at ${vp.width}`).toBeLessThanOrEqual(1);
  }
});
