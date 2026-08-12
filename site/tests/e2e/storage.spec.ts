import { test, expect } from '@playwright/test';
import { clickUntil } from './helpers';

// Storage (#storage) — segmented toggle switches the whole panel between the
// SQLite (solo) and PostgreSQL (shared team, WICKED_RUNTIME profile) views.

test('storage segmented toggle switches SQLite and PostgreSQL views', async ({ page }) => {
  await page.goto('/');
  const section = page.locator('#storage');
  await section.scrollIntoViewIfNeeded();

  const cmd = section.locator('.font-mono').filter({ hasText: 'wicked-estate index' });
  await expect(cmd).toHaveText(/graph\.db/); // SQLite is the default

  const pgBtn = section.locator('button.seg').filter({ hasText: 'PostgreSQL' });
  await clickUntil(pgBtn, async () => /WICKED_RUNTIME=team/.test((await cmd.textContent()) ?? ''));

  // the team view is the WICKED_RUNTIME profile story
  await expect(pgBtn).toHaveAttribute('data-on', 'true');
  await expect(section.getByText('WICKED_STORE_URL=postgres://team/graph', { exact: false })).toBeVisible();
  await expect(section.getByText('concurrent — the whole team writes')).toBeVisible();
  await expect(section.getByText('--features postgres', { exact: false })).toBeVisible();

  // and back to SQLite
  await section.locator('button.seg').filter({ hasText: 'SQLite' }).click();
  await expect(cmd).toHaveText(/graph\.db/);
  await expect(section.getByText('single-writer — no daemon, no races')).toBeVisible();
});
