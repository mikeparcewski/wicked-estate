import { test, expect } from '@playwright/test';
import { clickUntil } from './helpers';

// Storage (#storage): the toggle switches the table between SQLite (solo) and Postgres (team).

test('storage toggle switches SQLite and PostgreSQL views', async ({ page }) => {
  await page.goto('/');
  const section = page.locator('#storage');
  await section.scrollIntoViewIfNeeded();

  const cmd = section.locator('[data-storage-cmd]');
  await expect(cmd).toHaveText(/graph\.db/);

  const pgBtn = section.locator('button.seg').filter({ hasText: 'PostgreSQL' });
  await clickUntil(pgBtn, async () => /WICKED_RUNTIME=team/.test((await cmd.textContent()) ?? ''));
  await expect(pgBtn).toHaveAttribute('data-on', 'true');
  await expect(section).toContainText('the whole team, concurrently');
  await expect(section).toContainText('refuses to start rather than fall back');

  await section.locator('button.seg').filter({ hasText: 'SQLite' }).click();
  await expect(cmd).toHaveText(/graph\.db/);
  await expect(section).toContainText('one writer, no daemon');
});
