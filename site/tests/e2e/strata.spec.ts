import { test, expect } from '@playwright/test';
import { clickUntil } from './helpers';

// FiveStrata (#strata) — auto-scans the five bands on a 1.9s interval;
// clicking a band pins it (stops the scan and locks the highlight).

test('FiveStrata: clicking a band pins it', async ({ page }) => {
  await page.goto('/');
  const section = page.locator('#strata');
  await section.scrollIntoViewIfNeeded();

  const pill = section.locator('.demo-pill');
  await expect(pill).toHaveText(/Auto-scanning/);
  await expect(pill).toHaveAttribute('data-live', 'true');

  const band = section.locator('.rock-panel .cursor-pointer').filter({ hasText: 'Injected edges' });
  await clickUntil(band, async () => ((await pill.textContent()) ?? '').includes('Pinned'));

  // pinned state: pill flips, and the clicked band carries the active seam
  await expect(pill).toHaveText(/Pinned · click to resume scan/);
  await expect(pill).toHaveAttribute('data-live', 'false');
  await expect(band).toHaveAttribute('style', /inset 3px/);

  // clicking the pill resumes the scan
  await pill.click();
  await expect(pill).toHaveText(/Auto-scanning/);
});
