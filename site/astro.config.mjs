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
  // Astro's default HTML compression deletes the space before an inline tag (<b>, <code>), fusing
  // words on screen ("callsapplyDiscount"); the family's standing rule is to keep it off.
  compressHTML: false,
  site: SITE,
  base: BASE,
  trailingSlash: 'ignore',
  integrations: [react()],
  vite: {
    // Safari before 16.4 ignores media-query range syntax (`width <= 760px`), so the minifier must keep min-width and
    // max-width: the CSS targets name the oldest browsers the site supports (wicked-web#63, wickedagile#71).
    build: { cssTarget: ['chrome100', 'edge100', 'firefox100', 'safari14'] },
    plugins: [tailwindcss()],
    resolve: { alias: wickedWebAlias },
    define: { __WICKED_ESTATE_VERSION__: JSON.stringify(estateVersion) },
    optimizeDeps: {
      include: ['react-dom/client'],
    },
  },
});
