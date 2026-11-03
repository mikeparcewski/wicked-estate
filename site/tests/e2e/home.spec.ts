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
    // one browser-side sequence, all inside one glide's window: a notch starts a glide to the stores, the
    // keyboard takes over and the page jumps to its end, then a notch goes back up. It must start from where
    // the page is, not from the old glide target (which would ask for the hero)
    const asked = await page.evaluate(() => {
      const w = window as unknown as { __to: number[] };
      const wheel = (dy: number) => window.dispatchEvent(new WheelEvent('wheel', { deltaY: dy, deltaMode: 0, cancelable: true }));
      wheel(100);
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'End' }));
      // the End key's own jump (an instant scroll; a scrollTop write would not cut into the running glide)
      window.scrollTo({ top: document.documentElement.scrollHeight, behavior: 'instant' });
      wheel(-100);
      return w.__to.slice();
    });
    expect(asked[0]).toBe(t.stores);
    expect(asked.at(-1), 'the notch went back from the old glide target').toBeGreaterThan(t.tools);
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

// The snap guard never traps a reader. Locked: every scene fits (give or take its own bottom padding), across the
// laptop heights that used to overshoot (776-864 tall, the hero's svh paddings against a fixed playground).
// Unlocked: no snap at all (not the chrome's proximity, which pulls each notch back to a one-frame scene).
for (const vp of [{ width: 1470, height: 840 }, { width: 1440, height: 789 }, { width: 1536, height: 864 }, { width: 1366, height: 820 }, { width: 1366, height: 648 }]) {
  test(`the page locks in at ${vp.width}x${vp.height}, and a notch leaves the hero`, async ({ page }) => {
    await page.setViewportSize(vp);
    await page.goto('/');
    await expect(page.locator('html')).toHaveClass(/snap-lock/);
    // the hero itself fits, independent of the guard's slack (its playground subtracts the hero's own paddings)
    const hero = await page.locator('#hero').evaluate((el) => el.getBoundingClientRect().height - innerHeight);
    expect(hero, 'the hero runs past its frame').toBeLessThanOrEqual(0.5);
    const stores = await page.locator('#stores').evaluate((el) => Math.round(el.getBoundingClientRect().top + scrollY - 64));
    await page.mouse.move(vp.width / 2, vp.height / 2);
    await page.mouse.wheel(0, 100);
    await expect.poll(() => page.evaluate(() => Math.round(scrollY))).toBe(stores);
  });
}

test('where the lock cannot hold, the page does not snap, so a notch always scrolls', async ({ page }) => {
  // 1280x600 is under the lock's 640px floor. The pointer rests over the playground's answer panel, a scroller:
  // unlocked, it must hand the wheel on to the page once it is at its end
  await page.setViewportSize({ width: 1280, height: 600 });
  await page.goto('/');
  await expect(page.locator('html')).not.toHaveClass(/snap-lock/);
  expect(await page.evaluate(() => getComputedStyle(document.documentElement).scrollSnapType)).toBe('none');
  await page.mouse.move(640, 300);
  const ys: number[] = [];
  for (let i = 0; i < 5; i++) {
    await page.mouse.wheel(0, 100);
    await page.waitForTimeout(400);
    ys.push(await page.evaluate(() => Math.round(scrollY)));
  }
  // the panel may take a notch for its own spare scroll; every notch after that moves the page
  expect(ys[4], `the page held still or snapped back: ${ys.join(',')}`).toBeGreaterThanOrEqual(300);
});

test('a swipe inside the tools list moves only the list, both ways', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 700 });
  await page.goto('/');
  await expect(page.locator('html')).toHaveClass(/snap-lock/);
  const t = page.locator('#toolface');
  const y0 = await t.evaluate((el) => Math.round(el.getBoundingClientRect().top + scrollY - 64));
  await page.evaluate((y) => window.scrollTo({ top: y, behavior: 'instant' }), y0);
  // the island must be hydrated for the search to filter: retry the fill until the explorer is in search mode
  const search = t.getByLabel('Search tools');
  await expect.poll(async () => { await search.fill('e'); return t.locator('.tools').getAttribute('data-set'); }).toBe('search');
  const list = t.locator('.tools-list');
  await expect.poll(() => list.evaluate((el) => el.scrollHeight > el.clientHeight + 40), { message: 'the list must scroll for this check' }).toBe(true);
  const b = await list.boundingBox();
  await page.mouse.move(b!.x + 120, b!.y + 120);
  // one continuous swipe: 40 small deltas, 16ms apart, longer than the list (it reaches its end mid-gesture)
  for (let i = 0; i < 40; i++) { await page.mouse.wheel(0, 40); await page.waitForTimeout(16); }
  await page.waitForTimeout(900);
  expect(await list.evaluate((el) => el.scrollTop + el.clientHeight >= el.scrollHeight - 1), 'the list reached its end').toBe(true);
  expect(await page.evaluate(() => Math.round(scrollY)), 'the page flipped a scene in the same gesture').toBe(y0);
  for (let i = 0; i < 40; i++) { await page.mouse.wheel(0, -40); await page.waitForTimeout(16); }
  await page.waitForTimeout(900);
  expect(await list.evaluate((el) => el.scrollTop)).toBe(0);
  expect(await page.evaluate(() => Math.round(scrollY)), 'the page flipped a scene upward in the same gesture').toBe(y0);
});

test('the calm loops run only in the scene on screen', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 700 });
  await page.goto('/');
  const loops = () => page.evaluate(() => {
    const by: Record<string, number> = {};
    for (const a of document.getAnimations()) {
      const t = (a.effect as KeyframeEffect | null)?.target as Element | null;
      if (!t || a.effect?.getComputedTiming().iterations !== Infinity) continue;
      const sc = t.closest('[data-scene], .same-garden');
      const k = sc ? sc.id || 'garden' : 'none';
      by[k] = (by[k] ?? 0) + 1;
    }
    return by;
  });
  // the hero's pulse starts once its intro is over
  await expect.poll(async () => Object.keys(await loops())).toEqual(['hero']);
  await page.evaluate(() => window.scrollTo({ top: document.getElementById('provenance')!.getBoundingClientRect().top + scrollY - 64, behavior: 'instant' }));
  await expect.poll(async () => Object.keys(await loops())).toEqual(['provenance']);
});

test('the copy names only what estate ships and returns', async ({ page }) => {
  await page.goto('/');
  const text = await page.locator('main').innerText();
  // no LSP tier (its consumer is not wired), no memory fields recall does not return, jobs not runs
  expect(text).not.toMatch(/\bLSP\b/);
  expect(text).not.toMatch(/\b(told or witnessed|witnessed|kind: fact)\b/);
  expect(text).not.toMatch(/\br-412\b/);
  expect(text).not.toContain('sendReceipt');
  // the follow pill says it is an example, and the hero says what the page follows
  await expect(page.locator('#stores .follow')).toContainText('example: applyDiscount');
  await expect(page.locator('#query .q-bar')).toContainText('This page follows one function, applyDiscount, through all three stores.');
  // the first screen has a next step
  await expect(page.locator('.facts-go a')).toHaveAttribute('href', '#get-started');
  // the emit edge leaves the FILE (a TOML rule attaches it there), in the hero and in the event-bus picture
  await expect(page.locator('#query .q-graph .q-node--file + text')).toHaveText('checkout.ts');
  // geometry, not labels: the injected emit segment starts on the file node's box and ends on the topic's
  const g = await page.locator('#query .q-graph').evaluate((svg) => {
    const seg = svg.querySelector('.q-seg[data-from="checkout.ts"][data-to="order.paid"] line.q-edge') as SVGLineElement | null;
    const box = (cls: string, label: string) => {
      const t = [...svg.querySelectorAll(`.${cls} + text`)].find((x) => x.textContent === label);
      const r = t?.previousElementSibling as SVGRectElement | null;
      return r ? { x: r.x.baseVal.value, y: r.y.baseVal.value, w: r.width.baseVal.value, h: r.height.baseVal.value } : null;
    };
    const near = (px: number, py: number, b: { x: number; y: number; w: number; h: number } | null) =>
      !!b && px >= b.x - 4 && px <= b.x + b.w + 4 && py >= b.y - 4 && py <= b.y + b.h + 4;
    if (!seg) return { seg: false, starts: false, ends: false, dashed: false };
    const [x1, y1, x2, y2] = ['x1', 'y1', 'x2', 'y2'].map((k) => (seg as unknown as Record<string, SVGAnimatedLength>)[k].baseVal.value);
    return { seg: true, starts: near(x1, y1, box('q-node--file', 'checkout.ts')), ends: near(x2, y2, box('q-node--topic', 'order.paid')), dashed: seg.classList.contains('q-edge--inj') };
  });
  expect(g).toEqual({ seg: true, starts: true, ends: true, dashed: true });
  await expect(page.locator('#provenance .pv-svg--h .q-node-t')).toContainText(['price.ts', 'checkout.ts', 'applyDiscount', 'order.paid', 'receipt.ts', 'ledger.ts']);
  // What's new lists only the three released 0.30-0.31 items
  await expect(page.locator('#stores .news li')).toHaveCount(3);
});
