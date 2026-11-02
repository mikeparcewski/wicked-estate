import { test, expect } from '@playwright/test';
import { clickUntil } from './helpers';

// Storage (#storage, "Local by default."): the toggle switches the diagram and the table between
// SQLite (solo) and Postgres (team). Both states are laid out at once (one shown), so the scene
// never changes size: assertions read what is visible (innerText), not the DOM text.

test('storage toggle switches SQLite and PostgreSQL views, at one size', async ({ page }) => {
  await page.goto('/');
  const section = page.locator('#storage');
  await section.scrollIntoViewIfNeeded();
  await expect(section.locator('h2')).toHaveText('Local by default.');
  const h0 = await section.evaluate((el) => Math.round(el.getBoundingClientRect().height));

  const cmd = section.locator('[data-storage-cmd]');
  await expect(cmd).toHaveText(/graph\.db/, { useInnerText: true });

  const pgBtn = section.locator('button.seg').filter({ hasText: 'PostgreSQL' });
  await clickUntil(pgBtn, async () => /WICKED_RUNTIME=team/.test(await cmd.innerText()));
  await expect(pgBtn).toHaveAttribute('data-on', 'true');
  await expect(pgBtn).toHaveAttribute('aria-pressed', 'true');
  const table = section.locator('.store-t');
  await expect(table).toContainText('the whole team, concurrently', { useInnerText: true });
  await expect(table).toContainText('refuses to start rather than fall back', { useInnerText: true });
  // Team mode shares the code graph only: memory and knowledge stay on local SQLite.
  await expect(table).toContainText('still local SQLite; not shared yet', { useInnerText: true });
  await expect(section.locator('.sd')).toHaveAttribute('data-mode', 'team');
  await expect(section.locator('.sd-refuse')).toBeVisible();
  expect(await section.evaluate((el) => Math.round(el.getBoundingClientRect().height))).toBe(h0);

  await section.locator('button.seg').filter({ hasText: 'SQLite' }).click();
  await expect(cmd).toHaveText(/graph\.db/, { useInnerText: true });
  await expect(table).toContainText('one writer, no daemon', { useInnerText: true });
  await expect(section.locator('.sd-refuse')).toBeHidden();
});
