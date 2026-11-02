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
  await expect(table).toContainText('refuses team mode rather than fall back', { useInnerText: true });
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

// Criterion 6: no state moves a cylinder into its neighbour. graph.db fades where it stands in team mode, and the
// shared box shows the team picture as a dimmed outline in solo.
for (const vp of [{ width: 1440, height: 700 }, { width: 1280, height: 660 }, { width: 1920, height: 1080 }, { width: 390, height: 844 }]) {
  test(`storage: the cylinders keep their places and a 10px gap in both modes at ${vp.width}x${vp.height}`, async ({ page }) => {
    await page.setViewportSize(vp);
    await page.goto('/');
    const section = page.locator('#storage');
    await section.scrollIntoViewIfNeeded();
    const boxes = () => section.locator('.sd-box--local .cyl').evaluateAll((els) => els.map((e) => {
      const r = e.getBoundingClientRect();
      return { l: Math.round(r.left), r: Math.round(r.right), t: Math.round(r.top) };
    }));
    const solo = await boxes();
    const pgBtn = section.locator('button.seg').filter({ hasText: 'PostgreSQL' });
    await clickUntil(pgBtn, async () => (await section.locator('.sd').getAttribute('data-mode')) === 'team');
    // let the 0.3s fade finish
    await page.waitForTimeout(450);
    const team = await boxes();
    expect(team, 'a cylinder moved when switching to team').toEqual(solo);
    for (const set of [solo, team]) {
      for (let i = 1; i < set.length; i++) expect(set[i].l - set[i - 1].r, 'labels closer than 10px').toBeGreaterThanOrEqual(10);
    }
    // solo shows the shared graph as an outline, not an empty panel
    await section.locator('button.seg').filter({ hasText: 'SQLite' }).click();
    await expect(section.locator('.sd-shared-graph')).toBeVisible();
    expect(Number(await section.locator('.sd-shared-graph').evaluate((el) => getComputedStyle(el).opacity))).toBeGreaterThan(0.2);
  });
}
