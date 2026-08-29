import { test, expect } from '@playwright/test';

// Baseline smoke: the page renders every signature section with zero JS errors.
// This is the floor a redesign must clear before anything fancier.

test('home page renders hero, all signature sections, and footer without page errors', async ({ page }) => {
  const pageErrors: Error[] = [];
  page.on('pageerror', (e) => pageErrors.push(e));

  await page.goto('/');

  await expect(page).toHaveTitle(/wicked-estate/);

  // hero — the foundation thesis + the live core-log panel
  await expect(page.locator('h1')).toContainText('One binary');
  await expect(page.getByText('CORE LOG · applyDiscount')).toBeVisible();

  // every signature section is present and rendered
  for (const id of ['#query', '#strata', '#toolface', '#binary', '#provenance', '#storage', '#foundation', '#get-started']) {
    await expect(page.locator(id)).toBeVisible();
  }

  // #binary — the released long tail: watch mode, SCIP tier, multi-repo, IaC drift
  const binary = page.locator('#binary');
  await expect(binary).toContainText('The CLI ships the long tail');
  for (const verb of ['watch', 'scip', 'cross-graph', 'tfstate', 'drift', 'by-requirement', 'fingerprint']) {
    await expect(binary.getByText(verb, { exact: true }).first()).toBeVisible();
  }

  // #foundation is the shared SameGarden four-plane map, with estate marked
  // as "you are here" on the Foundation plane (never a self-promoting link)
  const map = page.locator('#foundation .same-garden');
  await expect(map).toBeVisible();
  const here = map.locator('.sg-card--here');
  await expect(here).toHaveCount(1);
  await expect(here).toContainText('wicked-estate');
  await expect(here.locator('.sg-here-chip')).toHaveText('you are here');

  // shared chrome — topbar wordmark + footer
  await expect(page.locator('.topbar .wordmark')).toBeVisible();
  await expect(page.locator('footer.footer')).toBeVisible();

  expect(pageErrors).toEqual([]);
});
