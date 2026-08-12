import { test, expect } from '@playwright/test';
import { clickUntil } from './helpers';

// Storage (#storage) — segmented toggle switches the whole panel between the
// SQLite (solo) and PostgreSQL (shared team) views.

test('storage segmented toggle switches SQLite and PostgreSQL views', async ({ page }) => {
  await page.goto('/');
  const section = page.locator('#storage');
  await section.scrollIntoViewIfNeeded();

  const cmd = section.locator('.font-mono').filter({ hasText: 'wicked-estate index' });
  await expect(cmd).toHaveText(/graph\.db/); // SQLite is the default

  const pgBtn = section.locator('button.seg').filter({ hasText: 'PostgreSQL' });
  await clickUntil(pgBtn, async () => /postgres:\/\/team\/graph/.test((await cmd.textContent()) ?? ''));

  await expect(pgBtn).toHaveAttribute('data-on', 'true');
  await expect(section.getByText('concurrent — the whole team writes')).toBeVisible();
  await expect(section.getByText('--features postgres')).toBeVisible();

  // and back to SQLite
  await section.locator('button.seg').filter({ hasText: 'SQLite' }).click();
  await expect(cmd).toHaveText(/graph\.db/);
  await expect(section.getByText('single-writer — one local file')).toBeVisible();
});
