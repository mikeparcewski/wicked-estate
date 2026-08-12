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
  // Keep the last click failure so a timeout reports WHY the click never
  // landed (detached node, overlay intercept, ...) instead of a bare poll
  // timeout.
  let lastClickError: unknown;
  try {
    await expect
      .poll(
        async () => {
          if (await effect()) return true;
          try {
            await target.click({ timeout: 2_000 });
            lastClickError = undefined;
          } catch (err) {
            lastClickError = err;
          }
          return effect();
        },
        { timeout },
      )
      .toBe(true);
  } catch (err) {
    if (lastClickError !== undefined) {
      throw new Error(
        `clickUntil on ${String(target)} timed out; last click failure: ${String(lastClickError)}`,
        { cause: err },
      );
    }
    throw err;
  }
}
