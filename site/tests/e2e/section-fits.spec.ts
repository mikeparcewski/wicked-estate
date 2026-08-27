import { test, expect, type Page } from '@playwright/test';

/**
 * Every stratum has to fit the screen it snaps to.
 *
 * WHAT WAS WRONG (issue #123). The page is a snap-per-screen deck: `html` sets
 * `scroll-snap-type: y proximity` and every `.strata` section is sized to one visible band
 * (`min-height: calc(100svh - var(--topbar-h))`). Measured on the deployed build at 1440x700,
 * four sections were taller than that band — 700, 713, 786 and 642 against 636px of usable
 * height, the worst 150px past the fold. #121 had reported only two of them, by 12px and 86px,
 * because it measured against the RAW viewport (700) instead of the usable height (636).
 *
 * THREE WAYS THIS MEASUREMENT GOES WRONG, all of which have actually happened on this page
 * family, and all of which this file is written to avoid:
 *
 * 1. MEASURING AGAINST THE RAW VIEWPORT. The topbar is `position: fixed` and overlays the page;
 *    `html` sets `scroll-padding-top: var(--topbar-h)`, so a snapped section only ever gets
 *    viewport MINUS the bar. Comparing to the viewport is ~64px too generous — exactly the
 *    mistake #121 made and #122 then had to correct in the shared chrome. The bar is measured,
 *    not hardcoded, so a token change in wicked-web cannot silently loosen this.
 *
 * 2. MEASURING THE RENDERED BOX. `min-height` clamps the box, so a section reports "fits, 0px
 *    spare" whether it has room to breathe or is one word away from overflowing — and then jumps
 *    past the budget in one step. These tests measure CONTENT (children + their margins + the
 *    section's own padding) and require real slack.
 *
 * 3. FORGETTING scroll-margin-top. `#query` sets one (it is the hero's "Read the record ↓"
 *    anchor target and wants a gap under the bar). That margin pushes the snapped section
 *    further down the screen, so it comes straight out of the section's height budget.
 *
 * WHY THESE VIEWPORTS. 1440x700 is the size the issue measured. 1280x660 is a 13" laptop, the
 * one that actually hurts. 1440x900 is taller than a real laptop window and hides all of this —
 * every one of the four over-tall sections "fits" at 900.
 */

const VIEWPORTS = [
  { width: 1440, height: 700 },
  { width: 1280, height: 660 },
];

/**
 * CI's Linux runner renders this page family taller than macOS does — wicked-studio measured
 * ~24px on a comparable hero — so "content <= budget" passing locally proves nothing. Require
 * real slack instead. Measured on this page at the time of writing, the tightest section has
 * 27px at 1280x660 and 58px at 1440x700.
 */
const MIN_SLACK = 20;

/** The fixed topbar, measured rather than assumed. Ceil, so a 64.4px bar cannot round down and
 *  overstate usable height by 0.4px: every rounding here is in the direction that makes the
 *  assertion stricter. Fails loudly rather than falling back to 0, which would quietly hand
 *  every section an extra 64px of budget if the wicked-web markup ever drifts. */
async function topbarHeight(page: Page): Promise<number> {
  const h = await page.evaluate(() => {
    const bar =
      document.getElementById('themeBtn')?.closest('header, .topbar') ??
      document.querySelector('.topbar, header[class*="topbar"]');
    return bar ? Math.ceil(bar.getBoundingClientRect().height) : 0;
  });
  expect(h, 'could not find the topbar to measure — the wicked-web selector has drifted').toBeGreaterThan(0);
  return h;
}

interface Stratum {
  id: string;
  /** rendered height — clamped by the min-height, so it says nothing about headroom */
  box: number;
  /** children + their margins + the section's own padding */
  content: number;
  /** the band `min-height` sizes the section to: 100svh, or 100svh − topbar */
  band: number;
  /** what the CONTENT may use: the band, less any scroll-margin-top */
  budget: number;
  hero: boolean;
  scrollMargin: number;
}

/** One pass over every `.strata` section, returning what it costs and what it is allowed. */
async function strata(page: Page, viewportHeight: number, barHeight: number): Promise<Stratum[]> {
  return page.evaluate(
    ({ vh, bar }) => {
      const usable = vh - bar;
      return [...document.querySelectorAll<HTMLElement>('section.strata')].map((el) => {
        const st = getComputedStyle(el);

        // Content = the section's own vertical padding + every in-flow child's border box and
        // margins. Absolutely positioned children are decoration (the seam line, the drill
        // underline, the gesture cursor) and are out of flow, so they cost the section nothing.
        let content = parseFloat(st.paddingTop) + parseFloat(st.paddingBottom);
        for (const child of [...el.children] as HTMLElement[]) {
          const cs = getComputedStyle(child);
          if (cs.position === 'absolute' || cs.position === 'fixed') continue;
          content +=
            child.getBoundingClientRect().height +
            parseFloat(cs.marginTop) +
            parseFloat(cs.marginBottom);
        }

        // The hero is the exception, and it is a real one rather than a fudge: it sits at scroll
        // position 0, where no scroll-padding applies and the fixed bar OVERLAYS the content
        // instead of pushing it down. So it is sized to the full 100svh (`.strata--hero`) and
        // clears the bar with its own top padding, which is already counted above.
        const hero = el.classList.contains('strata--hero');
        const scrollMargin = parseFloat(st.scrollMarginTop) || 0;
        const band = hero ? vh : usable;

        return {
          id: el.id || (hero ? '(hero)' : '(unnamed section)'),
          box: Math.ceil(el.getBoundingClientRect().height),
          content: Math.ceil(content),
          band,
          // scroll-margin-top does NOT shrink the rendered box (min-height doesn't know about
          // it) — it shifts the whole section down the screen, so it comes out of what the
          // CONTENT can use, and only out of that.
          budget: band - scrollMargin,
          hero,
          scrollMargin,
        };
      });
    },
    { vh: viewportHeight, bar: barHeight },
  );
}

for (const vp of VIEWPORTS) {
  test.describe(`strata at ${vp.width}x${vp.height}`, () => {
    test.use({ viewport: vp });

    test('every stratum has content headroom inside one snapped screen', async ({ page }) => {
      await page.goto('/');
      // Before the webfonts swap in, text is laid out in the fallback face and a section can
      // measure SHORTER than it finally renders — which would pass this test on a page that
      // does not actually fit. A size check that can silently pass early is worse than none.
      await page.evaluate(async () => { await document.fonts.ready; });

      const bar = await topbarHeight(page);
      const sections = await strata(page, vp.height, bar);

      // Guard the guard: if the selector ever stops matching, an empty list would make every
      // assertion below vacuously true and this file would go green on a broken page.
      expect(sections.length, 'no .strata sections found — the selector has drifted').toBe(7);

      for (const s of sections) {
        const slack = s.budget - s.content;
        const margin = s.scrollMargin ? ` (${s.scrollMargin}px of it spent on scroll-margin-top)` : '';
        expect(
          slack,
          `${s.id} content is ${s.content}px in ${s.budget}px of budget${margin} at ` +
            `${vp.width}x${vp.height} — only ${slack}px of slack, and CI's Linux runner renders ` +
            `this page family taller than macOS. Recover the height from type, spacing or copy; ` +
            `do not raise .strata's min-height.`,
        ).toBeGreaterThanOrEqual(MIN_SLACK);
      }
    });

    test('no stratum renders taller than the band it snaps to', async ({ page }) => {
      // The blunt version of the check above, stated the way the issue stated it. It is
      // redundant while the content check passes, and that is the point: if a future change
      // makes the two disagree, the content walk has stopped seeing part of the section.
      await page.goto('/');
      await page.evaluate(async () => { await document.fonts.ready; });

      const bar = await topbarHeight(page);
      for (const s of await strata(page, vp.height, bar)) {
        expect(
          s.box,
          `${s.id} renders ${s.box}px against a ${s.band}px band at ` +
            `${vp.width}x${vp.height} — ${s.box - s.band}px past the fold`,
        ).toBeLessThanOrEqual(s.band);
      }
    });

    test('the shared platform band fits too', async ({ page }) => {
      // wicked-web's SameGarden four-plane map. It rendered 1142px here before chrome #28/#30,
      // and a site only picks those up by re-pinning the commit in package.json AND
      // package-lock.json — so this is what catches a stale pin.
      await page.goto('/');
      await page.evaluate(async () => { await document.fonts.ready; });

      const band = page.locator('.same-garden');
      await expect(band).toHaveCount(1);

      const bar = await topbarHeight(page);
      const usable = vp.height - bar;
      const h = await band.evaluate((el) => Math.ceil(el.getBoundingClientRect().height));
      expect(
        h,
        `.same-garden is ${h}px in ${usable}px of usable height at ${vp.width}x${vp.height} — ` +
          `check that wicked-web is re-pinned in package.json AND package-lock.json`,
      ).toBeLessThanOrEqual(usable);
    });

    test('the sections that now fit are still snap targets', async ({ page }) => {
      // Fitting by turning the deck off would satisfy every assertion above while deleting the
      // behaviour they exist to protect. Snapping must still be on, and on every stratum.
      await page.goto('/');
      await page.evaluate(async () => { await document.fonts.ready; });

      // Chromium serializes `y proximity` back as just `y` — proximity is the initial
      // strictness — so assert on what it does say: an axis, and not `none`.
      const snapType = await page.evaluate(
        () => getComputedStyle(document.documentElement).scrollSnapType,
      );
      expect(snapType, 'the page stopped snapping').toMatch(/^y\b/);
      expect(snapType, 'snapping went back to mandatory — see #121, it froze the page').not.toContain('mandatory');

      const aligns = await page.evaluate(() =>
        [...document.querySelectorAll('section.strata')].map(
          (el) => getComputedStyle(el).scrollSnapAlign.split(' ')[0],
        ),
      );
      expect(aligns.length).toBeGreaterThan(0);
      expect(new Set(aligns), 'a stratum stopped being a snap target').toEqual(new Set(['start']));
    });
  });
}
