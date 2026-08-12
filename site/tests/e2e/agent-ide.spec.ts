import { test, expect } from '@playwright/test';
import { clickUntil } from './helpers';

// AgentIDE (#query) — the signature widget. Auto-play is IntersectionObserver-
// driven (threshold 0.2) and steps a storyboard of IDE gestures every ~2.65s;
// the confidence dial gates dock results; any interaction "takes control"
// (driving=true) and stops the auto-demo.

test.describe('AgentIDE widget', () => {
  test('auto-play starts when the section scrolls into view', async ({ page }) => {
    await page.goto('/');
    await page.locator('#query .ide-window').scrollIntoViewIfNeeded();

    // the auto-demo pill reports the demo is live
    await expect(page.locator('#query .ide-run')).toHaveAttribute('data-live', 'true');

    // storyboard activity is observable without any interaction from us:
    // the gesture cursor appears once the IntersectionObserver fires…
    await expect(page.locator('#query .ide-cursor')).toBeVisible({ timeout: 15_000 });
    // …the context menu opens by itself (step 0 opens it ~620ms in)…
    await expect(page.locator('#query .ide-ctxmenu')).toBeVisible({ timeout: 15_000 });
    // …and step 1 clicks the class, landing the all-details peek in the dock.
    await expect(page.locator('#query .ide-dock-title')).toHaveText('PricingService — all details', { timeout: 15_000 });
  });

  test('threshold dial changes the displayed cutoff and gates results', async ({ page }) => {
    await page.goto('/');
    const dial = page.locator('#query input.ide-dial');
    const shown = page.locator('#query .ide-gutter-val');
    await dial.scrollIntoViewIfNeeded();

    await expect(shown).toHaveText('0.55'); // SSR default

    // hydration-resilient: keep nudging until React reflects the new value
    // (End on a focused range input snaps to max = 1.00)
    await expect
      .poll(async () => {
        await dial.click({ timeout: 2_000 }).catch(() => {});
        await dial.press('End', { timeout: 2_000 }).catch(() => {});
        return shown.textContent();
      }, { timeout: 15_000 })
      .toBe('1.00');

    // interacting with the dial takes control of the demo
    await expect(page.locator('#query .ide-run')).toHaveAttribute('data-live', 'false');

    // at the max cutoff, sub-1.0 results fall out (rendered gated, labeled)
    await expect(page.locator('#query .ide-dock-body [data-on="false"]').first()).toBeVisible();

    // Home snaps back to the floor
    await dial.press('Home');
    await expect(shown).toHaveText('0.30');
  });

  test('clicking a symbol takes control and shows its full dossier', async ({ page }) => {
    await page.goto('/');
    const pill = page.locator('#query .ide-run');
    await pill.scrollIntoViewIfNeeded();
    await expect(pill).toHaveText(/Auto-demo/);

    const sym = page.locator('#query .ide-code button.ide-sym').filter({ hasText: 'PricingService' });
    await clickUntil(sym, async () => ((await pill.textContent()) ?? '').includes('Driving'));

    // control taken: auto-demo stopped, cursor overlay gone
    await expect(pill).toHaveAttribute('data-live', 'false');
    await expect(page.locator('#query .ide-cursor')).toHaveCount(0);

    // clicking the class lands "all details" in the Code intelligence tab
    await expect(page.locator('#query .ide-dock-title')).toHaveText('PricingService — all details');
    await expect(page.locator('#query #ide-tab-code')).toHaveAttribute('aria-selected', 'true');
  });
});
