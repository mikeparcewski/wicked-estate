import { test, expect } from '@playwright/test';

// Baseline smoke: the page renders every section with zero JS errors.

test('home page renders the hook, the playground, every section, and the footer', async ({ page }) => {
  const pageErrors: Error[] = [];
  page.on('pageerror', (e) => pageErrors.push(e));

  await page.goto('/');
  await expect(page).toHaveTitle(/wicked-estate/);

  // The hook says what your agents know (N10), and the playground sits in the hero.
  await expect(page.locator('h1')).toHaveText('What your agents know about your system.');
  await expect(page.locator('#query')).toBeVisible();

  for (const id of ['#why', '#stores', '#provenance', '#toolface', '#storage', '#foundation', '#get-started']) {
    await expect(page.locator(id)).toBeVisible();
  }

  // #foundation is the shared SameGarden map with estate as "you are here".
  const map = page.locator('#foundation .same-garden');
  await expect(map).toBeVisible();
  const here = map.locator('.sg-card--here');
  await expect(here).toHaveCount(1);
  await expect(here).toContainText('wicked-estate');
  await expect(here.locator('.sg-here-chip')).toHaveText('you are here');

  await expect(page.locator('.topbar .wordmark')).toBeVisible();
  await expect(page.locator('footer.footer')).toBeVisible();

  // No stale release callouts: the version comes from Cargo.toml at build time only.
  await expect(page.locator('body')).not.toContainText('v0.15.0');

  expect(pageErrors).toEqual([]);
});

test('the hero, playground included, fits a 1440x700 laptop window', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 700 });
  await page.goto('/');
  await page.evaluate(async () => { await document.fonts.ready; });
  const bottom = await page.locator('#query').evaluate((el) => el.getBoundingClientRect().bottom);
  expect(Math.round(bottom), 'the playground runs below the fold').toBeLessThanOrEqual(700);
  await expect(page.locator('.q-ask').first()).toBeInViewport({ ratio: 1 });
  await expect(page.locator('.q-tab').first()).toBeInViewport({ ratio: 1 });
  // The proof line is the first thing in the answer, above the rows, and all three edges show in the frame.
  await expect(page.locator('#query .q-note')).toContainText('grep finds 2 call sites');
  await expect(page.locator('#query .q-note')).toBeInViewport({ ratio: 1 });
  for (let i = 0; i < 3; i++) await expect(page.locator('#query .q-row').nth(i)).toBeInViewport({ ratio: 1 });
});

for (const vp of [{ width: 1440, height: 700 }, { width: 1280, height: 660 }, { width: 1920, height: 1080 }]) {
  test(`every scene fits its frame at ${vp.width}x${vp.height}, so the page locks in scene by scene`, async ({ page }) => {
    await page.setViewportSize(vp);
    await page.goto('/');
    await page.evaluate(async () => { await document.fonts.ready; });
    const rows = await page.evaluate(() => {
      const bar = 64;
      return [...document.querySelectorAll('main [data-scene], main .same-garden')].map((s) => {
        const h = s.getBoundingClientRect().height;
        const frame = s.classList.contains('strata--hero') ? innerHeight : innerHeight - bar;
        return { id: s.id || s.className.toString(), h: Math.round(h), frame };
      });
    });
    for (const r of rows) expect(r.h, `${r.id} is taller than its frame`).toBeLessThanOrEqual(r.frame + 1);
    // scenes.ts upgrades to mandatory snap only when every scene fits
    await expect(page.locator('html')).toHaveClass(/snap-lock/);
  });
}

test.describe('the wheel pages scene by scene under the snap lock', () => {
  // under mandatory snap, Chromium pulls a small wheel step back to the scene it left, so without the
  // stepper (scenes.ts) a mouse could never leave the hero
  const tops = async (page: import('@playwright/test').Page) => {
    await page.setViewportSize({ width: 1440, height: 700 });
    // record every scroll the stepper asks for, so a dropped or stale request shows even when the page lands
    await page.addInitScript(() => {
      const w = window as unknown as { __to: number[] };
      w.__to = [];
      const orig = window.scrollTo.bind(window);
      window.scrollTo = ((a?: ScrollToOptions | number, b?: number) => {
        if (typeof a === 'object' && a && typeof a.top === 'number') w.__to.push(Math.round(a.top));
        return typeof a === 'number' ? orig(a, b ?? 0) : orig(a);
      }) as typeof window.scrollTo;
    });
    await page.goto('/');
    await expect(page.locator('html')).toHaveClass(/snap-lock/);
    await page.mouse.move(700, 400);
    const top = (id: string) => page.locator(id).evaluate((el) => Math.round(el.getBoundingClientRect().top + scrollY - 64));
    return { stores: await top('#stores'), prov: await top('#provenance'), tools: await top('#toolface') };
  };
  const y = (page: import('@playwright/test').Page) => () => page.evaluate(() => Math.round(scrollY));

  test('one notch moves one scene, both ways', async ({ page }) => {
    const t = await tops(page);
    await page.mouse.wheel(0, 100);
    await expect.poll(y(page)).toBe(t.stores);
    await page.waitForTimeout(800);
    await page.mouse.wheel(0, 100);
    await expect.poll(y(page)).toBe(t.prov);
    await page.waitForTimeout(800);
    await page.mouse.wheel(0, -100);
    await expect.poll(y(page)).toBe(t.stores);
  });

  test('notches during a glide still count, and a reversal comes back', async ({ page }) => {
    const t = await tops(page);
    const asked = () => page.evaluate(() => (window as unknown as { __to: number[] }).__to);
    await page.mouse.wheel(0, 100);
    await page.waitForTimeout(200);
    await page.mouse.wheel(0, 100);
    await expect.poll(y(page)).toBe(t.prov);
    await page.waitForTimeout(150);
    await page.mouse.wheel(0, 100);
    await page.waitForTimeout(150);
    await page.mouse.wheel(0, -100);
    // each notch asked for its own scene: down, down, down to the tools, then back from the tools' glide
    await expect.poll(asked).toEqual([t.stores, t.prov, t.tools, t.prov]);
    await expect.poll(y(page)).toBe(t.prov);
  });

  test('another way of moving drops a glide\'s target', async ({ page }) => {
    const t = await tops(page);
    // keys jump at once here, so the whole exchange sits inside one glide's window
    await page.addStyleTag({ content: 'html{scroll-behavior:auto!important}' });
    const end = await page.evaluate(() => document.documentElement.scrollHeight - innerHeight);
    await page.mouse.wheel(0, 100);
    await page.waitForTimeout(60);
    // the keyboard takes the page on (End), then the wheel goes back up: from where the page is, not from the
    // glide's old target (that would land on the hero)
    await page.keyboard.press('End');
    await expect.poll(y(page)).toBe(end);
    await page.mouse.wheel(0, -100);
    await expect.poll(y(page)).toBeGreaterThan(t.tools);
  });

  test('a small pixel step moves a scene; a dense stream (a swipe and its momentum) moves only one', async ({ page }) => {
    const t = await tops(page);
    await page.mouse.wheel(0, 4);
    await expect.poll(y(page)).toBe(t.stores);
    await page.waitForTimeout(800);
    for (let i = 0; i < 20; i++) await page.mouse.wheel(0, 60);
    await expect.poll(y(page)).toBe(t.prov);
    await page.waitForTimeout(400);
    expect(await y(page)(), 'the stream ran on past one scene').toBe(t.prov);
  });
});

test('the page never scrolls sideways', async ({ page }) => {
  for (const vp of [{ width: 1440, height: 900 }, { width: 390, height: 844 }]) {
    await page.setViewportSize(vp);
    await page.goto('/');
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    expect(overflow, `horizontal overflow at ${vp.width}`).toBeLessThanOrEqual(1);
  }
});

test('the sub-line and benefits say a local CLI and MCP server, never one binary', async ({ page }) => {
  await page.goto('/');
  // Two executables ship (wicked-estate, wicked-estate-mcp); team Postgres is an optional CLI build.
  await expect(page.locator('.hero-head p')).toContainText('a local CLI and MCP server');
  // This page's own sections only: the shared family map (#foundation) takes its blurbs from the
  // pinned wicked-web, which carries the same fix in its own PR and arrives with a re-pin.
  const own = await page.evaluate(() =>
    Array.from(document.querySelectorAll('main section'))
      .filter((s) => !s.closest('#foundation'))
      .map((s) => (s as HTMLElement).innerText)
      .join(' '),
  );
  expect(own).not.toMatch(/one (local |static )?binary/i);
  // Benefits 1 and 2 head the stores they come from; benefit 3 is the storage scene's headline.
  const why = page.locator('#why');
  await expect(why.locator('.ben')).toHaveCount(2);
  await expect(why.locator('.ben').first()).toContainText('Know what breaks before you change it.');
  await expect(page.locator('#storage h2')).toHaveText('Local by default.');
  await expect(page.locator('#storage')).toContainText('the MCP server, memory and knowledge do not use it yet');
  await expect(why.locator('[data-news] li')).toHaveCount(3);
  // the unqualified storage headline is retired
  await expect(page.locator('body')).not.toContainText('SQLite by default. Postgres for a team.');
});
