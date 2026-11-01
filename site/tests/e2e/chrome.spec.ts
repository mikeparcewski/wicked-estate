import { test, expect } from '@playwright/test';

// Universal chrome (wicked-web Topbar): theme toggle + ecosystem dropdown.
// Both are inline scripts (no React hydration involved).

test('theme toggle flips data-theme on <html> and persists via localStorage', async ({ page }) => {
  await page.goto('/');
  const html = page.locator('html');
  await expect(html).toHaveAttribute('data-theme', 'dark'); // default: Wicked Dark
  // loading a page persists nothing: only a click writes the choice
  expect(await page.evaluate(() => localStorage.getItem('wa-theme-v3'))).toBeNull();

  await page.locator('#themeBtn').click();
  await expect(html).toHaveAttribute('data-theme', 'light');
  await expect
    .poll(() => page.evaluate(() => localStorage.getItem('wa-theme-v3')))
    .toBe('light');

  // persists across reload (no-flash init in Base.astro reads wa-theme-v3)
  await page.reload();
  await expect(html).toHaveAttribute('data-theme', 'light');

  // and toggles back
  await page.locator('#themeBtn').click();
  await expect(html).toHaveAttribute('data-theme', 'dark');
  await expect
    .poll(() => page.evaluate(() => localStorage.getItem('wa-theme-v3')))
    .toBe('dark');
});

test('ecosystem dropdown opens on click and closes on Escape', async ({ page }) => {
  await page.goto('/');
  const btn = page.locator('#projectsBtn');
  const menu = page.locator('#projectsMenu');

  await expect(menu).toBeHidden();
  await btn.click();
  await expect(menu).toBeVisible();
  await expect(btn).toHaveAttribute('aria-expanded', 'true');

  // The four-plane roster is intact: 4 marketed items — studio · crew · garden · estate.
  // 4, not 5: the ecosystem dropdown no longer carries a wicked-interactive row. Its builder UI
  // moved into wicked-studio and the service answers a direct visitor with "it serves the API, not
  // the UI", so a nav row there would spend the click telling you that. The product is not gone —
  // SameGarden lists it under Foundation as the document engine, repo link and no "Visit" link.
  await expect(menu.locator('a.dropdown-item')).toHaveCount(4);
  await expect(menu.locator('a.dropdown-item').filter({ hasText: 'estate' })).toBeVisible();

  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();
  await expect(btn).toHaveAttribute('aria-expanded', 'false');
});
