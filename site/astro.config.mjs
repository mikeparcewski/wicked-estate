// @ts-check
import { defineConfig } from 'astro/config';
import react from '@astrojs/react';
import tailwindcss from '@tailwindcss/vite';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

// Shared chrome from the `wicked-web` package: local source when it sits beside
// this repo (../../wicked-web) for live dev, else the installed git package in CI.
const localUI = fileURLToPath(new URL('../../wicked-web/src', import.meta.url));
/** @type {Record<string, string>} */
const wickedWebAlias = existsSync(localUI) ? { 'wicked-web': localUI } : {};

/**
 * Deploy target: custom domain https://we.wickedagile.com (CNAME in public/),
 * served at the root, so base is "/". Overridable via env if ever needed.
 */
const SITE = process.env.SITE_URL ?? 'https://we.wickedagile.com';
const BASE = process.env.BASE_PATH ?? '/';

export default defineConfig({
  site: SITE,
  base: BASE,
  trailingSlash: 'ignore',
  integrations: [react()],
  vite: {
    plugins: [tailwindcss()],
    resolve: { alias: wickedWebAlias },
    optimizeDeps: {
      include: ['react-dom/client'],
    },
  },
});
