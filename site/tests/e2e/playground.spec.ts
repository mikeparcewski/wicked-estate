import { test, expect } from '@playwright/test';
import { clickUntil } from './helpers';

// The query playground (#query): pick a question, see which store answers, with provenance.

test.describe('query playground', () => {
  test('opens on a blast-radius answer from the code graph, with the guess held back', async ({ page }) => {
    await page.goto('/');
    const q = page.locator('#query');
    await expect(q.locator('.q-call')).toContainText('BlastRadius');
    await expect(q.locator('.q-tab[data-store="graph"]')).toHaveAttribute('aria-selected', 'true');
    // Three edges at 0.5 or above; the 0.3 tag-scan guess is held back by default.
    await expect(q.locator('.q-row')).toHaveCount(3);
    await expect(q.locator('.q-hidden')).toContainText('1 guess below 0.5 held back');
    await expect(q.locator('.q-row').filter({ hasText: 'sendReceipt()' })).toContainText('injected');
    await expect(q.getByRole('radio', { name: '0.5' })).toHaveAttribute('aria-checked', 'true');
  });

  test('the confidence control is one value: the hero and the confidence scene move together', async ({ page }) => {
    await page.goto('/');
    const q = page.locator('#query');
    const low = q.getByRole('radio', { name: '0.3' });
    await clickUntil(low, async () => (await q.locator('.q-row').count()) === 4);
    const guess = q.locator('.q-row').filter({ hasText: 'referralFlow()' });
    await expect(guess).toContainText('Tags');
    await expect(guess).toContainText('0.3');
    await expect(guess).toContainText('tag-scan guess, unverified');
    await expect(q.locator('.q-hidden')).toHaveCount(0);
    // the slider in the confidence scene shows the same value, and the guess is named there
    const pv = page.locator('#provenance');
    const slider = pv.getByRole('slider');
    await expect(slider).toHaveValue('0');
    await expect(pv.locator('.thr-guess')).toBeVisible();
    // and back the other way: the slider at 1.0 leaves only the two exact edges in the hero
    await slider.scrollIntoViewIfNeeded();
    await slider.fill('2');
    await expect(pv.locator('.thr-v')).toHaveText('1.0');
    await expect(pv.locator('.tiers tr[data-off]')).toHaveCount(4);
    await expect(q.locator('.q-row')).toHaveCount(2);
    await expect(q.getByRole('radio', { name: '1.0' })).toHaveAttribute('aria-checked', 'true');
  });

  test('arrow keys move between the questions', async ({ page }) => {
    await page.goto('/');
    const q = page.locator('#query');
    // a verified selection change first, so the keys land on a hydrated island
    const stack = q.getByRole('option', { name: 'Can two coupons stack?' });
    await clickUntil(stack, async () => (await q.getAttribute('data-q')) === 'stack');
    await stack.focus();
    await page.keyboard.press('ArrowDown');
    await expect(q).toHaveAttribute('data-q', 'all');
    await expect(q.getByRole('option', { name: 'Everything about applyDiscount' })).toBeFocused();
    await page.keyboard.press('ArrowUp');
    await expect(q).toHaveAttribute('data-q', 'stack');
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
    // 12 graph tools + SemanticSearch (listed only with embeddings, badged "+1"); 11 memory; 7 knowledge: 30.
    await expect(count('Graph')).toHaveText('12 +1');
    await expect(count('Memory')).toHaveText('11');
    await expect(count('Knowledge')).toHaveText('7');
    await expect(t.locator('.tl-row')).toHaveCount(13);
    await expect(t.locator('.tl-row').filter({ hasText: 'SemanticSearch' })).toContainText('when enabled');

    const mem = t.locator('.tools-tab').filter({ hasText: 'Memory' });
    await clickUntil(mem, async () => (await mem.getAttribute('aria-selected')) === 'true');
    await expect(t.locator('.tl-row .tl-name')).toContainText(['memory.capture', 'memory.recall']);

    await t.getByLabel('Search tools').fill('erase');
    await expect(t.locator('.tl-row')).toHaveCount(1);
    await expect(t.locator('.tl-row')).toContainText('memory.erase');
    await t.getByLabel('Search tools').fill('drift');
    await expect(t.locator('.tl-row')).toContainText('drift');
    await t.getByLabel('Search tools').fill('zzzz');
    await expect(t).toContainText('No tool matches');
  });

  test('the explorer keeps one frame: tabs and searches never change its height', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 700 });
    await page.goto('/');
    const t = page.locator('#toolface');
    await t.scrollIntoViewIfNeeded();
    const height = () => t.evaluate((el) => Math.round(el.getBoundingClientRect().height));
    const nextTop = () => page.locator('#storage').evaluate((el) => Math.round(el.getBoundingClientRect().top + scrollY));
    const h0 = await height();
    const n0 = await nextTop();
    const cli = t.locator('.tools-tab').filter({ hasText: 'CLI' });
    await clickUntil(cli, async () => (await cli.getAttribute('aria-selected')) === 'true');
    expect(await height()).toBe(h0);
    for (const q of ['e', 'erase', 'zzz']) {
      await t.getByLabel('Search tools').fill(q);
      expect(await height(), `search "${q}"`).toBe(h0);
    }
    expect(await nextTop()).toBe(n0);
    expect(h0).toBeLessThanOrEqual(700 - 64);
  });

  test('"See it answer" plays that tool\'s question in the playground', async ({ page }) => {
    await page.goto('/');
    const t = page.locator('#toolface');
    await t.scrollIntoViewIfNeeded();
    const mem = t.locator('.tools-tab').filter({ hasText: 'Memory' });
    await clickUntil(mem, async () => (await mem.getAttribute('aria-selected')) === 'true');
    await expect(t.locator('.td-name')).toContainText('memory.recall');
    await t.getByRole('button', { name: /See it answer/ }).click();
    const q = page.locator('#query');
    await expect(q).toHaveAttribute('data-q', 'why');
    await expect(q.locator('.q-tab[data-store="memory"]')).toHaveAttribute('aria-selected', 'true');
    await expect(q).toBeInViewport();
  });
});

test.describe('scenes', () => {
  test('a store card opens its applyDiscount preview in place, without changing size', async ({ page }) => {
    await page.goto('/');
    const card = page.locator('#stores .sc[data-store="memory"]');
    await card.scrollIntoViewIfNeeded();
    const box0 = await card.boundingBox();
    const btn = card.locator('.sc-btn');
    await clickUntil(btn, async () => (await btn.getAttribute('aria-pressed')) === 'true');
    await expect(card.locator('.sc-face--prev')).toBeVisible();
    await expect(card.locator('.sc-face--prev')).toContainText('A discount never takes a total below zero.');
    await expect(card.locator('.sc-face--about')).toBeHidden();
    const box1 = await card.boundingBox();
    expect(box1?.height).toBe(box0?.height);
  });

  test('grep sees two call sites; estate adds the injected order.paid edges', async ({ page }) => {
    await page.goto('/');
    const pv = page.locator('#provenance');
    await pv.scrollIntoViewIfNeeded();
    const grep = pv.getByRole('button', { name: 'grep', exact: true });
    await clickUntil(grep, async () => (await grep.getAttribute('aria-pressed')) === 'true');
    await expect(pv.locator('.pv-read p:not([data-off])')).toContainText('grep finds 2 call sites.');
    await expect(pv.locator('.toml')).not.toHaveAttribute('data-hot', 'true');
    // the diagram: two call edges always; the three injected order.paid edges only in estate mode
    const diagram = pv.locator('.pv-svg--h');
    await expect(diagram).toBeVisible();
    await expect(diagram.locator('.pv-e--call')).toHaveCount(2);
    const inj = diagram.locator('.pv-e--inj');
    await expect(inj).toHaveCount(3);
    const opacities = () => inj.evaluateAll((els) => els.map((e) => getComputedStyle(e).opacity));
    await expect.poll(opacities).toEqual(['0', '0', '0']);
    await pv.getByRole('button', { name: 'estate', exact: true }).click();
    await expect(pv.locator('.pv-read p:not([data-off])')).toContainText('The graph finds a third through an event.');
    await expect(pv.locator('.pv-read p:not([data-off])')).toContainText('rule: event-bus-consume');
    await expect(pv.locator('.toml')).toHaveAttribute('data-hot', 'true');
    await expect.poll(opacities).toEqual(['1', '1', '1']);
  });

  test('a store card enters even when the window is too short to show a share of the scene', async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 320 });
    await page.goto('/');
    const card = page.locator('#stores .sc[data-store="knowledge"]');
    await card.scrollIntoViewIfNeeded();
    await expect.poll(() => card.evaluate((el) => getComputedStyle(el).opacity), { timeout: 6_000 }).toBe('1');
  });
});
