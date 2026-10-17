// @ts-check
import { defineConfig } from 'astro/config';
import react from '@astrojs/react';
import tailwindcss from '@tailwindcss/vite';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

// Shared chrome from the `wicked-web` package: local source when it sits beside
// this repo (../../wicked-web) for live dev, else the installed git package in CI.
// The workspace version from ../Cargo.toml, injected at build time so the site's version stamp
// can never trail the release it describes.
const cargo = readFileSync(fileURLToPath(new URL('../Cargo.toml', import.meta.url)), 'utf8');
const estateVersion = (cargo.match(/^\[workspace\.package\][^[]*?^version\s*=\s*"([^"]+)"/m) ?? cargo.match(/^version\s*=\s*"([^"]+)"/m))[1];

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
    define: { __WICKED_ESTATE_VERSION__: JSON.stringify(estateVersion) },
    optimizeDeps: {
      include: ['react-dom/client'],
    },
  },
});
