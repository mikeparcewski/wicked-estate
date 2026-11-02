import { test, expect } from '@playwright/test';

/**
 * Text and code faces (wicked-web v3): words are Wicked Sans, code is JetBrains Mono at the code step.
 * A tag that holds a tool name (<code class="tag">) is code; a provenance chip (Parsed, injected,
 * told, cited) is a label.
 */
test('code tags are mono at 13px; provenance chips are the text face', async ({ page }) => {
  await page.goto('/');
  const codeTag = page.locator('code.tag').first();
  await expect(codeTag).toHaveCSS('font-family', /JetBrains Mono/);
  await expect(codeTag).toHaveCSS('font-size', '13px');
  const chip = page.locator('.q-chip').first();
  await expect(chip).toBeVisible();
  await expect(chip).toHaveCSS('font-family', /^"?Wicked Sans/);
  await expect(chip).toHaveCSS('font-size', '12px');
});
