import { test, expect } from '@playwright/test';

// Universal chrome (wicked-web Topbar): theme toggle, the product path ("Wicked Agile / labs / wicked-estate")
// and the flask that links Wicked Labs. The theme toggle is an inline script (no React hydration involved).

test('theme toggle flips data-theme on <html> and persists via localStorage', async ({ page }) => {
  await page.goto('/');
  const html = page.locator('html');
  await expect(html).toHaveAttribute('data-theme', 'dark'); // default: Wicked Dark
  // loading a page persists nothing: only a click writes the choice
  expect(await page.evaluate(() => localStorage.getItem('wa-theme-v3'))).toBeNull();

  await page.locator('#themeBtn').click();
  await expect(html).toHaveAttribute('data-theme', 'light');
  await expect
    .poll(() => page.evaluate(() => localStorage.getItem('wa-theme-v3')))
    .toBe('light');

  // persists across reload (no-flash init in Base.astro reads wa-theme-v3)
  await page.reload();
  await expect(html).toHaveAttribute('data-theme', 'light');

  // and toggles back
  await page.locator('#themeBtn').click();
  await expect(html).toHaveAttribute('data-theme', 'dark');
  await expect
    .poll(() => page.evaluate(() => localStorage.getItem('wa-theme-v3')))
    .toBe('dark');
});

test('the product path reads Wicked Agile / labs / wicked-estate, and the flask links Wicked Labs', async ({ page }) => {
  await page.goto('/');
  const path = page.getByRole('navigation', { name: 'Breadcrumb' });
  await expect(path.getByRole('link')).toHaveCount(3);
  await expect(path.getByRole('link', { name: 'Wicked Agile' })).toHaveAttribute('href', 'https://wickedagile.com/');
  await expect(path.getByRole('link', { name: 'labs' })).toHaveAttribute('href', 'https://wickedlabs.ai/');
  const here = path.getByRole('link', { name: 'wicked-estate' });
  await expect(here).toHaveAttribute('href', 'https://we.wickedagile.com');
  await expect(here).toHaveAttribute('aria-current', 'page');
  // the flask is a plain link now: no Experiments menu, no button that opens one
  const lab = page.locator('.topbar .nav').getByRole('link', { name: 'Wicked Labs' });
  await expect(lab).toHaveAttribute('href', 'https://wickedlabs.ai/');
  await expect(lab).toBeVisible();
  await expect(page.locator('#projectsBtn, #projectsMenu, .dropdown-menu')).toHaveCount(0);
});

// a phone shows the mark (clipped from the lockup), "labs" and the product, and the path never pushes the icons;
// where the product name does not fit (320px) it keeps the chrome's ellipsis
for (const width of [390, 360, 320]) {
  test(`at ${width}px the product path keeps its mark and ends before the icons; the menu's first row is Wicked Labs`, async ({ page }) => {
    await page.setViewportSize({ width, height: 740 });
    await page.goto('/');
    await page.evaluate(async () => { await document.fonts.ready; });
    const g = await page.evaluate(() => {
      const box = (s: string) => { const r = document.querySelector(s)!.getBoundingClientRect(); return { l: r.left, r: r.right }; };
      const name = document.querySelector<HTMLElement>('.crumbs li:last-child .crumb')!, ns = getComputedStyle(name);
      return { mark: box('.crumbs .wordmark'), art: box('.crumbs .wordmark .wordmark-lockup'), path: box('.crumbs'), nav: box('.topbar .nav'), vw: innerWidth,
               clip: getComputedStyle(document.querySelector('.crumbs .wordmark .wordmark-lockup')!).clipPath,
               name: name.textContent!.trim(), cut: name.scrollWidth > name.clientWidth + 1,
               ellipsis: ns.textOverflow === 'ellipsis' && ns.overflowX === 'hidden' && !/flex|grid/.test(ns.display),
               sep: box('.crumbs li:nth-child(2) .crumb-sep') };
    });
    // the 44px targets keep the chrome's spacing: the first slash 8px after the clipped mark (22px of artwork)
    expect(Math.abs(g.sep.l - (g.art.l + 22 + 8))).toBeLessThanOrEqual(1.5);
    // the mark is painted inside the window and inside its own link
    expect(g.mark.l).toBeGreaterThanOrEqual(0);
    expect(g.art.l).toBeGreaterThanOrEqual(0);
    expect(g.art.l).toBeGreaterThanOrEqual(g.mark.l);
    // a cut name ends in an ellipsis (a flex box would hard-cut it)
    if (g.cut) expect(g.ellipsis, 'the product name is cut without an ellipsis').toBe(true);
    expect(g.path.r).toBeLessThanOrEqual(g.nav.l);
    expect(g.nav.r).toBeLessThanOrEqual(g.vw);
    expect(g.name).toBe('wicked-estate');
    expect(g.clip).toMatch(/^inset\(.*22px/);
    await page.locator('#menuBtn').click();
    const first = page.locator('#mobileMenu a').first();
    await expect(first).toContainText('Wicked Labs');
    await expect(first).toHaveAttribute('href', 'https://wickedlabs.ai/');
    // every row's glyph sits on its label's line, not above it
    const off = await page.locator('#mobileMenu .mm-label').evaluateAll((labels) => labels.map((l) => {
      const gl = l.querySelector('.mm-glyph')!.getBoundingClientRect();
      const r = document.createRange(); r.selectNodeContents(l); const t = Array.from(r.getClientRects()).filter((q) => q.width > 0);
      const text = t[t.length - 1], mid = (gl.top + gl.bottom) / 2;
      return mid >= text.top && mid <= text.bottom ? null : `${l.textContent!.trim()}: glyph ${Math.round(gl.top)}-${Math.round(gl.bottom)}, text ${Math.round(text.top)}-${Math.round(text.bottom)}`;
    }).filter(Boolean));
    expect(off).toEqual([]);
  });
}
