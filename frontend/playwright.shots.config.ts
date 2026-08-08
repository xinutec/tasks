import { defineConfig, devices } from '@playwright/test';
import { phoneConfig } from '@xinutec/ui-harness/config';
import harness from './e2e/harness.mjs';

/**
 * The screenshot pass — `pnpm run shots`. Same geometry, same static server and
 * the same fixtures as `playwright.config.ts`; only the file it runs differs,
 * because Playwright picks its suite from `testMatch` and one config cannot
 * hold two.
 *
 * Kept out of the gate on purpose: it asserts nothing. Its output is for a
 * person to look at, and a check nobody reads is worse than none.
 */
export default defineConfig(phoneConfig(harness, devices, { testMatch: '**/shots.spec.ts' }));
