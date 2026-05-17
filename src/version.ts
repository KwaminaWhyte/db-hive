/**
 * Single source of truth for the displayed app version.
 *
 * The value is injected by Vite from `package.json` (see `vite.config.ts`
 * `define.__APP_VERSION__`). Bump the version in `package.json` only — every
 * UI surface that imports `APP_VERSION` updates automatically on rebuild.
 */
export const APP_VERSION: string = __APP_VERSION__;
