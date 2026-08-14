import { test } from '@playwright/test';
// The fleet-shared harness, published as @xinutec/ui-harness (source repo
// ~/Code/ui-harness). Ships compiled JS, so it loads straight from node_modules.
import {
  expectIconFontLoaded,
  expectNoHorizontalOverflow,
  expectNoTextOverlaps,
  expectNoClippedIcons,
  expectViewportIsPhone,
} from '@xinutec/ui-harness';

import { DROPPED, SESSIONS, TASKS, mockApi } from './fixtures';

/**
 * L2 phone-width layout harness. Render the real screens at a Pixel viewport
 * with the backend mocked and BUSY data, and assert the failure classes that
 * read fine in source and only show on a real phone: text that collides, and
 * anything spilling past the right edge.
 *
 * ⚠ **These checks are geometry, and geometry is not the whole of it.** Both
 * defects found on the first render of this app were invisible here: a chip
 * capped so tight that `memview` came out as `memv…`, and a two-line `mat-hint`
 * overflowing Material's one-line subscript slot onto the field below — text
 * over a *border*, which is not text over text. `shots.spec.ts` is the other
 * half, and it is why that one exists.
 */
test.use({ serviceWorkers: 'block' });

// The checker-checker: fail loudly here if the device preset is ever lost and
// the "phone width" suite silently runs at desktop width.
test('the suite really runs at phone geometry', async ({ page }) => {
  await mockApi(page);
  await page.goto('/');
  await expectViewportIsPhone(page);
});

test('the list — long subjects beside a holder chip @ phone width', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.goto('/');
  await page.getByText('Stop walking every transcript').waitFor();
  await expectIconFontLoaded(page);
  await expectNoTextOverlaps(page, testInfo);
  await expectNoHorizontalOverflow(page, testInfo, null, FILTER_SCROLLERS);
  // A holder chip is capped and told to ellipsise; clipped TEXT elsewhere is a
  // cap somebody added without meaning to.
  await expectNoClippedIcons(page, testInfo);
});

/**
 * The filter row scrolls sideways BY DESIGN. Wrapping it would push the list
 * itself below the fold on a phone — the one thing the screen is for.
 */
const FILTER_SCROLLERS = ['.filters .row'];

test('the list — filtered to one holder @ phone width', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'with a session' }).click();
  await page.getByText('The console task reader').waitFor();
  await expectNoTextOverlaps(page, testInfo);
  await expectNoHorizontalOverflow(page, testInfo, null, FILTER_SCROLLERS);
});

/**
 * Fenced code and wide tables in a task body scroll horizontally BY DESIGN — a
 * shell command must not be reflowed to be readable. They are named explicitly,
 * since the harness refuses to infer that from `overflow-x`.
 */
const MD_SCROLLERS = ['.md-content pre', '.md-content table'];

test('a task — a 200-character subject, prose, and a raw session id in the history @ phone width', async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.goto('/t/92');
  await page.getByRole('heading', { name: 'Abstractly evaluate', exact: false }).waitFor();
  // By role: a Material icon renders its ligature into the accessible tree, so
  // a text query for a common word can match an icon as well as the heading.
  await page.getByRole('heading', { name: 'History' }).waitFor();
  await expectNoTextOverlaps(page, testInfo);
  await expectNoHorizontalOverflow(page, testInfo, null, MD_SCROLLERS);
  await expectNoClippedIcons(page, testInfo);
});

test('a task — the move menu, with an unnamed session in it @ phone width', async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.goto('/t/92');
  await page.locator('.move').click();
  // The unnamed session is drawn as its 36-character id; the menu is where that
  // has the least room.
  await page.getByRole('menuitem', { name: SESSIONS[1].id }).waitFor();
  // ⚠ **Scoped to the overlay, not the page.** An open menu is *supposed* to
  // cover what is under it, so a whole-page overlap check reports every line of
  // the task body against every menu item — four "collisions" that are the
  // feature working. What is worth asserting is that the menu does not collide
  // with itself, and that a 36-character id does not push it off the screen.
  await expectNoTextOverlaps(page, testInfo, '.mat-mdc-menu-panel');
  await expectNoHorizontalOverflow(page, testInfo, null, MD_SCROLLERS);
});

test('a task — the overflow menu, whose one item is a sentence @ phone width', async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.goto('/t/92');
  await page.getByRole('button', { name: 'More actions' }).click();
  // The label is a phrase rather than a verb, because "drop" alone reads as
  // "delete" — and a phrase in a menu on a phone is where width runs out.
  await page.getByRole('menuitem', { name: 'Drop it' }).waitFor();
  await expectNoTextOverlaps(page, testInfo, '.mat-mdc-menu-panel');
  await expectNoHorizontalOverflow(page, testInfo, null, MD_SCROLLERS);
});

test('a dropped task — closed, and not counted as done @ phone width', async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.route('**/api/tasks/*', (r) => r.fulfill({ json: DROPPED }));
  await page.goto('/t/92');
  await page.getByText('dropped').first().waitFor();
  await expectNoTextOverlaps(page, testInfo);
  await expectNoHorizontalOverflow(page, testInfo, null, MD_SCROLLERS);
  await expectNoClippedIcons(page, testInfo);
});

test('filing a task — a hint under a field, and a label under that @ phone width', async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.goto('/new');
  await page.getByRole('heading', { name: 'File a task' }).waitFor();
  await page.getByLabel('Subject').fill(TASKS[1].subject);
  await expectNoTextOverlaps(page, testInfo);
  await expectNoHorizontalOverflow(page, testInfo);
  await expectNoClippedIcons(page, testInfo);
});
