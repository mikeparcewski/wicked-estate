import { expect, type Locator } from '@playwright/test';

/**
 * React islands hydrate async — a click before hydration is a silent no-op.
 * Retry the click until its observable effect holds (never a fixed sleep).
 * Also survives transient overlays (e.g. the AgentIDE context-menu backdrop
 * the auto-demo opens for ~1.2s at a time).
 */
export async function clickUntil(
  target: Locator,
  effect: () => Promise<boolean>,
  { timeout = 15_000 }: { timeout?: number } = {},
): Promise<void> {
  await target.scrollIntoViewIfNeeded();
  await expect
    .poll(
      async () => {
        if (await effect()) return true;
        await target.click({ timeout: 2_000 }).catch(() => {});
        return effect();
      },
      { timeout },
    )
    .toBe(true);
}
