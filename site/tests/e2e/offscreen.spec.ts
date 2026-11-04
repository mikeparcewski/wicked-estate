import { test, expect } from '@playwright/test';
import { SIZES, place, scrollFor, watch, read, waitForMotion, expectStill, setHidden, snaps } from './offscreen-kit';

/**
 * Every scene's motion stops when nobody can see it, and nothing moves the page
 * (operator, 2026-11-04: "when interactives are playing the scroll is scrolling back to them").
 *
 * The idle loops run only while their scene is live (data-live: 40% of the window or of the scene,
 * scenes.ts) and the tab is visible. For each scene, at 1440x900, 1280x720 and 390x844: it plays on
 * screen; off screen (and, where the page does not snap, peeking in at an edge) nothing in it
 * changes or animates for two of its cycles, the page holds still and nothing takes focus; back on
 * screen it plays again. The get-started typing holds while its scene is not live, and a hidden tab
 * stops the loops.
 */

const ITEMS = [
  { name: 'the hero (the answer rows)', sel: '#hero', cycle: 5200 },
  { name: 'the stores', sel: '#stores', cycle: 3400 },
  { name: 'provenance', sel: '#provenance', cycle: 5200 },
  { name: 'the tools', sel: '#toolface', cycle: 6000 },
  { name: 'storage', sel: '#storage', cycle: 4200 },
  { name: 'the family map', sel: '#foundation .same-garden', cycle: 3600 },
  { name: 'get started', sel: '#get-started', cycle: 4500 },
];

for (const size of SIZES) {
  test.describe(`${size.width}x${size.height}`, () => {
    test.use({ viewport: size });

    for (const it of ITEMS) {
      test(`${it.name}: its loops hold off screen and play again when it is back`, async ({ page }) => {
        test.setTimeout(90_000);
        await page.goto('/');
        await place(page, it.sel, 'on');
        const playing = await waitForMotion(page, it.sel, 4000, true);
        expect(playing.running, `${it.name} plays on screen`).toBeGreaterThan(0);
        expect(playing.moved, `${it.name}: the page moved while it played`).toBeLessThanOrEqual(1);
        await expectStill(page, it.sel, it.cycle, 'off', it.name);
        if (!(await snaps(page))) await expectStill(page, it.sel, it.cycle, 'edge', it.name);
        await place(page, it.sel, 'on');
        const again = await waitForMotion(page, it.sel, 4000, true);
        expect(again.running, `${it.name} plays again back on screen`).toBeGreaterThan(0);
        expect(again.moved, `${it.name}: the page moved while it played`).toBeLessThanOrEqual(1);
        expect(again.focusMoved, `${it.name}: focus moved while it played`).toBe(false);
      });
    }

    test('a hidden tab stops the loops of the scene on screen; visible again, they play', async ({ page }) => {
      test.setTimeout(45_000);
      await page.goto('/');
      await place(page, '#stores', 'on');
      await waitForMotion(page, '#stores', 4000, true);
      await setHidden(page, true);
      await watch(page, '#stores');
      await page.waitForTimeout(3000);
      const s = await read(page, '#stores');
      expect(s.running, 'animations running in a hidden tab').toBe(0);
      await setHidden(page, false);
      const again = await waitForMotion(page, '#stores', 4000, true);
      expect(again.running, 'the loops play once the tab is visible').toBeGreaterThan(0);
    });
  });
}

// letters typed so far in the terminal, and its whole length
const typing = (page: import('@playwright/test').Page) => page.evaluate(() => {
  const len = (sel: string) => Array.from(document.querySelectorAll(`#get-started .gs-term ${sel} .cmd-line`)).reduce((n, l) => n + (l.textContent ?? '').length, 0);
  return { typed: len('.cmd-type'), full: len('.cmd-full') };
});

for (const size of SIZES) {
  test.describe(`the get-started typing ${size.width}x${size.height}`, () => {
    test.use({ viewport: size });
    test('holds when its scene leaves mid-typing and finishes exactly once it is back', async ({ page }) => {
      test.setTimeout(45_000);
      await page.goto('/');
      const scene = '#get-started';
      await page.evaluate((sel) => {
        const s = document.querySelector(sel) as HTMLElement;
        window.scrollTo({ top: Math.round(s.getBoundingClientRect().top + window.scrollY), behavior: 'instant' as ScrollBehavior });
      }, scene);
      // a phone shows one install path at a time: the terminal types once its path is picked
      if (size.width <= 760) await page.locator(`${scene} .gs-views`).getByText('Or just estate').click();
      // the typing follows the terminal's own visibility: bring the terminal itself on screen
      const term = `${scene} .gs-term`;
      await page.locator(term).evaluate((el) => el.scrollIntoView({ block: 'center', behavior: 'instant' as ScrollBehavior }));
      // leave mid-typing (the commands take 1.35 s)
      await expect.poll(async () => { const t = await typing(page); return t.typed > 0 && t.typed < t.full; }, { timeout: 5000, intervals: [30] }).toBe(true);
      await scrollFor(page, scene, 'off');
      await page.waitForTimeout(300);
      const held = await typing(page);
      expect(held.typed, 'it left mid-typing').toBeLessThan(held.full);
      await watch(page, scene);
      await page.waitForTimeout(2000);
      expect((await typing(page)).typed, 'the typing held off screen').toBe(held.typed);
      expect((await read(page, scene)).mutations, 'nothing in the scene changed off screen').toBe(0);
      await place(page, term, 'on');
      await expect.poll(async () => { const t = await typing(page); return t.typed === t.full; }, { timeout: 6000, message: 'every command typed in full' }).toBe(true);
    });
  });
}
