import { test, expect, devices } from '@playwright/test';
import { clickUntil } from './helpers';

// Mobile (iPhone 12, 390x844): a deliberate phone layout. The five questions wrap (no hidden
// side-scroller), the demo stays labelled as scripted, picking a question never moves the page,
// the explorer still searches, tap targets are at least 44px, nothing scrolls sideways. Every frame
// fits one screen (360x740 too): the hero's answer, the confidence ladder and the storage details
// start a second frame; the stores, the tool groups and the install paths switch in place.

const { defaultBrowserType: _ignored, ...iphone12 } = devices['iPhone 12'];
test.use(iphone12);

test('"See it answer" lands on the answer, even in a short window', async ({ page }) => {
  // reduced motion: the landing is an instant jump, so the check reads where it settles, not a smooth
  // scroll passing through the viewport
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.setViewportSize({ width: 320, height: 320 });
  await page.goto('/');
  const t = page.locator('#toolface');
  await t.scrollIntoViewIfNeeded();
  const mem = t.locator('.tools-tab').filter({ hasText: 'Memory' });
  await clickUntil(mem, async () => (await mem.getAttribute('aria-selected')) === 'true');
  await t.getByRole('button', { name: /See it answer/ }).click();
  const q = page.locator('#query');
  await expect(q).toHaveAttribute('data-q', 'why');
  await expect(page.locator('#q-panel')).toBeFocused();
  const top = await q.locator('.q-card').first().evaluate((el) => Math.round(el.getBoundingClientRect().top));
  expect(top, 'the first answer card sits below the 64px topbar').toBeGreaterThanOrEqual(64);
  expect(top, 'the first answer card is on screen').toBeLessThan(320);
});

test('mobile: the playground and the tool explorer work, no horizontal overflow', async ({ page }) => {
  const pageErrors: Error[] = [];
  page.on('pageerror', (e) => pageErrors.push(e));
  await page.goto('/');
  await expect(page.locator('h1')).toBeVisible();
  await expect(page.locator('#menuBtn')).toBeVisible();

  const q = page.locator('#query');
  await expect(q.locator('.q-scripted')).toBeVisible();
  // all five questions are fully on screen horizontally: wrapped, not a side-scroller
  for (let i = 0; i < 5; i++) {
    const b = await q.locator('.q-ask').nth(i).boundingBox();
    expect(b && b.x >= 0 && b.x + b.width <= 390).toBe(true);
    expect(b && b.height >= 44).toBe(true);
  }
  const next = () => page.locator('#stores').evaluate((el) => Math.round(el.getBoundingClientRect().top + scrollY));
  const n0 = await next();
  const stack = q.getByRole('option', { name: 'Can two coupons stack?' });
  await clickUntil(stack, async () => (await q.getAttribute('data-q')) === 'stack');
  await expect(q.locator('.q-card').first()).toContainText('docs/pricing.md');
  await q.getByRole('option', { name: 'Everything about applyDiscount' }).click();
  await q.getByRole('option', { name: 'Who reacts when an order is paid?' }).click();
  expect(await next(), 'picking a question moved the page').toBe(n0);
  for (const sel of ['.q-tab', '.q-stop:visible']) {
    const b = await q.locator(sel).first().boundingBox();
    expect(b && b.height >= 40, `${sel} tap target`).toBe(true);
  }

  const t = page.locator('#toolface');
  await t.scrollIntoViewIfNeeded();
  await t.getByLabel('Search tools').fill('blast');
  await expect(t.locator('.tl-row')).toContainText('BlastRadius');

  for (const id of ['#stores', '#provenance', '#storage', '#foundation', '#get-started']) {
    await page.locator(id).scrollIntoViewIfNeeded();
    await expect(page.locator(id)).toBeVisible();
  }
  // "See it answer" lands on the answer, not on the questions stacked above it
  const mem = t.locator('.tools-tab').filter({ hasText: 'Memory' });
  await t.getByLabel('Search tools').fill('');
  await clickUntil(mem, async () => (await mem.getAttribute('aria-selected')) === 'true');
  await t.getByRole('button', { name: /See it answer/ }).click();
  await expect(q).toHaveAttribute('data-q', 'why');
  await expect(q.locator('.q-card').first()).toBeInViewport();

  const copy = await page.locator('#get-started .copy-btn').first().boundingBox();
  expect(copy && copy.height >= 44).toBe(true);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(1);
  expect(pageErrors).toEqual([]);
});

/** Frame heights (each piece from one landing point to the next) and the tops of every scene. */
async function frames(page: import('@playwright/test').Page) {
  return page.evaluate(() => {
    const bar = 64, Y = scrollY;
    const top = (s: string) => document.querySelector(s)!.getBoundingClientRect().top + Y;
    const h = (s: string) => document.querySelector(s)!.getBoundingClientRect().height;
    const inner = (s: string) => h(`${s} .scene-in`) + 32;
    const pieces: [string, number][] = [
      ['hero, first frame', top('#query .q-answer') - bar],
      ['hero, the answer', top('#hero') + h('#hero') - top('#query .q-answer') + 12],
      ['stores', inner('#stores')],
      ['provenance, first frame', top('#provenance .ph-frame') - top('#provenance')],
      ['provenance, confidence', top('#provenance') + h('#provenance') - top('#provenance .ph-frame') + 12],
      ['tools', inner('#toolface')],
      ['storage, first frame', top('#storage .ph-frame') - top('#storage')],
      ['storage, details', top('#storage') + h('#storage') - top('#storage .ph-frame') + 12],
      ['get started', inner('#get-started')],
    ];
    const tops = ['#stores', '#provenance', '#toolface', '#storage', '#get-started'].map((s) => Math.round(top(s)));
    return { frame: innerHeight - bar, pieces: pieces.map(([n, v]) => [n, Math.round(v)] as [string, number]), tops };
  });
}

for (const vp of [{ width: 390, height: 844 }, { width: 360, height: 740 }]) {
  test(`every phone frame fits one screen at ${vp.width}x${vp.height}, and no control moves the page`, async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.setViewportSize(vp);
    await page.goto('/');
    await page.evaluate(async () => { await document.fonts.ready; });
    const f0 = await frames(page);
    for (const [name, h] of f0.pieces) expect(h, `${name} is taller than the screen`).toBeLessThanOrEqual(f0.frame);
    const still = async (what: string) => {
      const f = await frames(page);
      expect(f.tops, `${what} moved the scenes below it`).toEqual(f0.tops);
      for (const [name, h] of f.pieces) expect(h, `${what}: ${name}`).toBeLessThanOrEqual(f.frame);
    };
    const q = page.locator('#query');
    // the tallest answers: the guess shown, and "Everything" in every store
    await clickUntil(q.locator('.q-stop:visible').last(), async () => (await q.locator('.q-row').count()) === 4);
    await still('the 0.3 stop');
    await q.getByRole('option', { name: 'Everything about applyDiscount' }).click();
    for (const tab of ['memory', 'knowledge', 'graph']) { await q.locator(`#q-tab-${tab}`).click(); await still(`Everything, ${tab}`); }
    const stores = page.locator('#stores');
    for (const v of ['Memory', 'Knowledge', 'What’s new', 'Code graph']) {
      await stores.locator('.ph-opt').filter({ hasText: v }).click(); await still(`stores: ${v}`);
    }
    const t = page.locator('#toolface');
    for (const tab of ['CLI', 'Memory', 'Graph']) {
      await t.locator('.tools-tab').filter({ hasText: tab }).click();
      for (let i = 0; i < 4; i++) { await t.getByRole('button', { name: 'Next group' }).click(); await still(`tools ${tab} group ${i}`); }
    }
    for (const term of ['e', 'zzz']) { await t.getByLabel('Search tools').fill(term); await still(`search "${term}"`); }
    await page.locator('#storage .seg').filter({ hasText: 'PostgreSQL' }).click(); await still('storage: team');
    await page.locator('#get-started .ph-opt').filter({ hasText: 'Or just estate' }).click(); await still('install: cargo');
  });
}

test('a tapped question glides to its answer frame', async ({ page }) => {
  await page.goto('/');
  const q = page.locator('#query');
  const stack = q.getByRole('option', { name: 'Can two coupons stack?' });
  await clickUntil(stack, async () => (await q.getAttribute('data-q')) === 'stack');
  await expect.poll(() => page.evaluate(() => scrollY), { message: 'the page stayed on the questions' }).toBeGreaterThan(300);
  await expect(q.locator('.q-asked')).toContainText('Can two coupons stack?');
  await expect(q.locator('.q-card').first()).toBeInViewport({ ratio: 1 });
});

test('the stores switch one store at a time, in one box', async ({ page }) => {
  await page.goto('/');
  const s = page.locator('#stores');
  await s.scrollIntoViewIfNeeded();
  const box = () => s.locator('.stores-grid').evaluate((el) => Math.round(el.getBoundingClientRect().height));
  const h0 = await box();
  await expect(s.locator('.sc[data-store="graph"]')).toBeVisible();
  await expect(s.locator('.sc[data-store="memory"]')).toBeHidden();
  const mem = s.locator('.ph-opt').filter({ hasText: 'Memory' });
  await clickUntil(mem, async () => (await mem.getAttribute('aria-pressed')) === 'true');
  await expect(s.locator('.sc[data-store="memory"]')).toBeVisible();
  await expect(s.locator('.ben--pair')).toBeVisible();
  await expect(s.locator('.sc[data-store="graph"]')).toBeHidden();
  await s.locator('.ph-opt').filter({ hasText: 'What’s new' }).click();
  await expect(s.locator('[data-news] li').first()).toBeVisible();
  expect(await box()).toBe(h0);
});

test('the tool explorer steps one group, or one page of matches, at a time', async ({ page }) => {
  await page.goto('/');
  const t = page.locator('#toolface');
  await t.scrollIntoViewIfNeeded();
  const list = () => t.locator('.tools-list').evaluate((el) => Math.round(el.getBoundingClientRect().height));
  await expect(t.locator('.tl-step-t')).toContainText('Find 1 of 4');
  await expect(t.locator('.tl-row:visible')).toHaveCount(4);
  const h0 = await list();
  const next = t.getByRole('button', { name: 'Next group' });
  await clickUntil(next, async () => /Follow 2 of 4/.test(await t.locator('.tl-step-t').innerText()));
  await expect(t.locator('.tl-row:visible .tl-name')).toHaveText(['BlastRadius', 'Lineage', 'Path', 'TraverseGraph']);
  await t.getByLabel('Search tools').fill('e');
  await expect(t.locator('.tl-step-t')).toContainText(/^1–3 of \d+$/);
  await expect(t.locator('.tl-hit:visible')).toHaveCount(3);
  await t.getByRole('button', { name: 'Next matches' }).click();
  await expect(t.locator('.tl-step-t')).toContainText(/^4–6 of \d+$/);
  expect(await list()).toBe(h0);
});
