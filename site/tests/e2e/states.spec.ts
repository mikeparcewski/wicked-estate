import { test, expect, type Page } from '@playwright/test';
import { clickUntil } from './helpers';

// Interaction states never disturb the page: focus does not move it, hover paints inside its own box, a
// tooltip shows whole, and a pane always describes what is in view.

/** scrollY once it has held still for three reads in a row (a smooth scroll or a snap may still be running). */
async function settled(page: Page) {
  let last = -1, same = 0;
  for (let i = 0; i < 60; i++) {
    const y = await page.evaluate(() => Math.round(scrollY));
    same = y === last ? same + 1 : 0;
    if (same >= 3) return y;
    last = y;
    await page.waitForTimeout(80);
  }
  return last;
}

for (const vp of [{ width: 1440, height: 700 }, { width: 1280, height: 660 }, { width: 1920, height: 1080 }]) {
  test(`keyboard focus on the tools tab, then a click elsewhere, leaves the page where it is at ${vp.width}x${vp.height}`, async ({ page }) => {
    await page.setViewportSize(vp);
    await page.goto('/');
    await expect(page.locator('html')).toHaveClass(/snap-lock/);
    const onTab = () => page.evaluate(() => !!document.activeElement?.classList.contains('tools-tab'));
    for (let i = 0; i < 80 && !(await onTab()); i++) await page.keyboard.press('Tab');
    expect(await onTab(), 'Tab never reached the tools tab').toBe(true);
    const y0 = await settled(page);
    const tools = await page.locator('#toolface').evaluate((el) => Math.round(el.getBoundingClientRect().top + scrollY - 64));
    expect(y0, 'focus did not bring the tools scene into view').toBe(tools);
    // blur: a click in the page's left gutter (nothing there takes focus)
    await page.mouse.click(4, Math.round(vp.height / 2));
    await expect.poll(() => page.evaluate(() => document.activeElement?.classList.contains('tools-tab'))).toBe(false);
    expect(await settled(page), 'blurring the tab moved the page').toBe(y0);
  });
}

test('a topbar tooltip shows whole, below its icon', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 700 });
  await page.goto('/');
  const btn = page.locator('#themeBtn');
  await btn.hover();
  const tip = await btn.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el, '::after');
    const top = r.top + parseFloat(cs.top);
    return { top, bottom: top + parseFloat(cs.height), btnBottom: r.bottom, content: cs.content };
  });
  expect(tip.content).toContain('toggle theme');
  expect(tip.top, 'the tooltip starts above the window').toBeGreaterThanOrEqual(0);
  expect(tip.top, 'the tooltip covers its own icon').toBeGreaterThanOrEqual(tip.btnBottom);
  expect(tip.bottom).toBeLessThanOrEqual(700);
});

test('a lit family plane glows inside its own box, never past it', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 700 });
  await page.goto('/');
  const plane = page.locator('#foundation .sg-plane').last();
  await plane.scrollIntoViewIfNeeded();
  await plane.hover({ position: { x: 40, y: 20 } });
  // every shadow layer of the lit plane is inset (an outer one paints over the seam text and the next section)
  await expect.poll(() => plane.evaluate((el) => {
    const s = getComputedStyle(el).boxShadow;
    if (s === 'none') return 'none';
    const layers = s.split(/,(?![^(]*\))/);
    return layers.every((l) => /\binset\b/.test(l)) ? 'inset' : s;
  })).toBe('inset');
});

test.describe('tool explorer search', () => {
  test('the detail names the first match, or no tool when nothing matches', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 700 });
    await page.goto('/');
    const t = page.locator('#toolface');
    await t.scrollIntoViewIfNeeded();
    const search = t.getByLabel('Search tools');
    await clickUntil(search, async () => (await search.evaluate((el) => el === document.activeElement)));
    await search.fill('erase');
    await expect(t.locator('.td-name')).toHaveText('memory.erase');
    await expect(t.locator('.tl-row[aria-pressed="true"]')).toHaveText(/memory\.erase/);
    await search.fill('zzzz');
    await expect(t.locator('.td-name')).toHaveCount(0);
    await expect(t.locator('.td-none')).toContainText('No tool matches “zzzz”');
    await expect(t.locator('.tl-row[aria-pressed="true"]')).toHaveCount(0);
    // a search that still holds the selected tool keeps it
    await search.fill('');
    await expect(t.locator('.td-name')).toHaveText('BlastRadius');
    await search.fill('blast');
    await expect(t.locator('.td-name')).toHaveText('BlastRadius');
  });
});

for (const vp of [{ width: 1440, height: 700 }, { width: 1280, height: 660 }, { width: 1920, height: 1080 }]) {
  const open = async (page: Page) => {
    await page.setViewportSize(vp);
    await page.goto('/');
    await page.evaluate(async () => { await document.fonts.ready; });
    const t = page.locator('#toolface');
    await t.scrollIntoViewIfNeeded();
    return t;
  };
  const tabs = ['Graph', 'Memory', 'Knowledge', 'CLI'];
  const pick = async (t: import('@playwright/test').Locator, tab: string) => {
    const b = t.locator('.tools-tab').filter({ hasText: tab });
    await clickUntil(b, async () => (await b.getAttribute('aria-selected')) === 'true');
  };

  test(`every tool name breaks only after a dot at ${vp.width}x${vp.height}`, async ({ page }) => {
    const t = await open(page);
    for (const tab of tabs) {
      await pick(t, tab);
      // each piece between two break points sits on one line: a piece on two lines broke mid-word
      const split = await t.locator('.tl-name').evaluateAll((els) => els.flatMap((el) => Array.from(el.childNodes)
        .filter((n) => n.nodeType === Node.TEXT_NODE && n.textContent)
        .filter((n) => { const r = document.createRange(); r.selectNodeContents(n); return new Set(Array.from(r.getClientRects()).map((x) => Math.round(x.top))).size > 1; })
        .map((n) => `${el.textContent}: "${n.textContent}"`)));
      expect(split, `${tab}: names broken mid-word`).toEqual([]);
    }
  });

  test(`no scripted-answer preview line is cut short, and every one fits its pane, at ${vp.width}x${vp.height}`, async ({ page }) => {
    const t = await open(page);
    for (const tab of tabs) {
      await pick(t, tab);
      for (const name of await t.locator('.tl-row .tl-see-dot').evaluateAll((els) => els.map((e) => e.closest('.tl-row')!.getAttribute('data-name')!))) {
        await t.locator(`.tl-row[data-name="${name}"]`).click();
        await expect(t.locator('.td-name')).toHaveText(name);
        const cut = await t.locator('.td-k, .td-v').evaluateAll((els) => els
          .filter((e) => e.scrollWidth > e.clientWidth + 1 || e.scrollHeight > e.clientHeight + 1 || getComputedStyle(e).textOverflow === 'ellipsis')
          .map((e) => e.textContent));
        expect(cut, `${name}: preview lines cut short`).toEqual([]);
        const over = await t.locator('.tools-detail').evaluate((el) => el.scrollHeight - el.clientHeight);
        expect(over, `${name}: the detail overflows its box`).toBeLessThanOrEqual(1);
      }
    }
  });
}

test('each confidence tier reads as its name, a space, then its note', async ({ page }) => {
  await page.goto('/');
  const rows = await page.locator('#provenance .tiers th').evaluateAll((ths) => ths.map((th) => ({
    text: th.textContent, want: `${th.querySelector('.tiers-t')!.textContent} ${th.querySelector('.tiers-who')!.textContent}`,
  })));
  expect(rows.length).toBeGreaterThan(0);
  for (const r of rows) expect(r.text, 'tier and note run together').toBe(r.want);
});
