import { test, expect } from '@playwright/test';
import { clickUntil } from './helpers';

// The query playground (#query): pick a question, see which store answers, with provenance.

test.describe('query playground', () => {
  test('opens on a blast-radius answer from the code graph, with the guess hidden', async ({ page }) => {
    await page.goto('/');
    const q = page.locator('#query');
    await expect(q.locator('.q-call')).toContainText('BlastRadius');
    await expect(q.locator('.q-tab[data-store="graph"]')).toHaveAttribute('aria-selected', 'true');
    // Three edges at 0.5 or above; the 0.3 tag-scan guess is hidden by default.
    await expect(q.locator('.q-row')).toHaveCount(3);
    await expect(q.locator('.q-hidden')).toContainText('1 guess below 0.5 hidden');
    await expect(q.locator('.q-row').filter({ hasText: 'sendReceipt()' })).toContainText('injected');
  });

  test('Show guesses reveals the low-confidence edge, labelled', async ({ page }) => {
    await page.goto('/');
    const q = page.locator('#query');
    const toggle = q.getByLabel('Show guesses');
    await clickUntil(toggle, async () => (await q.locator('.q-row').count()) === 4);
    const guess = q.locator('.q-row').filter({ hasText: 'referralFlow()' });
    await expect(guess).toContainText('Tags');
    await expect(guess).toContainText('0.3');
    await expect(q.locator('.q-hidden')).toHaveCount(0);
  });

  test('each question routes to the store that answers it', async ({ page }) => {
    await page.goto('/');
    const q = page.locator('#query');

    const why = q.getByRole('option', { name: 'Why can a discount never go below zero?' });
    await clickUntil(why, async () => (await q.getAttribute('data-q')) === 'why');
    await expect(q.locator('.q-call')).toContainText('memory.recall');
    await expect(q.locator('.q-tab[data-store="memory"]')).toHaveAttribute('aria-selected', 'true');
    await expect(q.locator('.q-card').first()).toContainText('told');
    await expect(q.locator('.q-card').first()).toContainText('project:shop');

    await q.getByRole('option', { name: 'Can two coupons stack?' }).click();
    await expect(q.locator('.q-call')).toContainText('knowledge.recall');
    await expect(q.locator('.q-tab[data-store="knowledge"]')).toHaveAttribute('aria-selected', 'true');
    await expect(q.locator('.q-card').first()).toContainText('docs/pricing.md');

    await q.getByRole('option', { name: 'Who reacts when an order is paid?' }).click();
    await expect(q.locator('.q-row')).toHaveCount(3);
    await expect(q.locator('.q-chip--inj')).toHaveCount(3);
    await expect(q.locator('.q-note')).toContainText('TOML rule');
  });

  test('"Everything" fills all three stores, and an empty store says so', async ({ page }) => {
    await page.goto('/');
    const q = page.locator('#query');
    const all = q.getByRole('option', { name: 'Everything about applyDiscount' });
    await clickUntil(all, async () => (await q.getAttribute('data-q')) === 'all');
    await expect(q.locator('.q-tab[data-store="graph"] .q-count')).toHaveText('4');
    await expect(q.locator('.q-tab[data-store="memory"] .q-count')).toHaveText('2');
    await expect(q.locator('.q-tab[data-store="knowledge"] .q-count')).toHaveText('2');
    await q.locator('.q-tab[data-store="knowledge"]').click();
    await expect(q.locator('.q-card')).toHaveCount(2);

    await q.getByRole('option', { name: 'Can two coupons stack?' }).click();
    await q.locator('.q-tab[data-store="memory"]').click();
    await expect(q.locator('.q-empty')).toBeVisible();
  });
});

test.describe('tool explorer', () => {
  test('groups switch, counts add to 30 MCP tools, and search spans every group', async ({ page }) => {
    await page.goto('/');
    const t = page.locator('#toolface');
    await t.scrollIntoViewIfNeeded();
    const count = (name: string) => t.locator('.tools-tab').filter({ hasText: name }).locator('.q-count');
    // 12 graph tools + SemanticSearch (listed only with embeddings) = 13 rows; 11 memory; 7 knowledge.
    await expect(count('Graph')).toHaveText('13');
    await expect(count('Memory')).toHaveText('11');
    await expect(count('Knowledge')).toHaveText('7');
    await expect(t.locator('.tools-row')).toHaveCount(13);

    const mem = t.locator('.tools-tab').filter({ hasText: 'Memory' });
    await clickUntil(mem, async () => (await mem.getAttribute('aria-selected')) === 'true');
    await expect(t.locator('.tools-row dt')).toContainText(['memory.capture', 'memory.recall']);

    await t.getByLabel('Search tools').fill('erase');
    await expect(t.locator('.tools-row')).toHaveCount(1);
    await expect(t.locator('.tools-row')).toContainText('memory.erase');
    await t.getByLabel('Search tools').fill('drift');
    await expect(t.locator('.tools-row')).toContainText('drift');
    await t.getByLabel('Search tools').fill('zzzz');
    await expect(t).toContainText('No tool matches');
  });
});
