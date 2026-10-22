import { expect, test } from '@playwright/test';
// The fleet's harness (`@xinutec/ui-harness`, ~/Code/ui-harness), shipped as compiled JS.
import {
  expectIconFontLoaded,
  expectNoHorizontalOverflow,
  expectCleanLayout,
  expectRecoversFromMissingBundle,
  expectUpInTheBar,
  expectViewportIsPhone,
} from '@xinutec/ui-harness';
import { routesFrom, screens } from '@xinutec/ui-harness/routes';

import type { Created, Updated } from '../src/app/models';
import { DETAIL, DROPPED, SESSIONS, TASKS, mockApi } from './fixtures';

/**
 * Phone-width layout checks: real screens at a Pixel viewport, mocked backend,
 * busy data, asserting collisions and right-edge overflow. Geometry isn't
 * everything; `shots.spec.ts` is the other half.
 */

// Fail if the device preset is lost and the suite runs at desktop width.

/** Every screen in the route table (`routes.json`), not only those a test opens. */
for (const { route, url } of screens(
  routesFrom('e2e/routes.json'),
  { id: String(TASKS[1].id) },
)) {
  test(`${route} lays out cleanly @ phone width`, async ({ page }, testInfo) => {
    await mockApi(page);
    await page.goto(url);
    await page.waitForLoadState('networkidle');
    // A task's markdown scrolls its code sideways, as the `/t/92` tests allow.
    await expectCleanLayout(page, testInfo, { allow: route === '/t/:id' ? MD_SCROLLERS : [] });
  });
}
test('the suite really runs at phone geometry', async ({ page }) => {
  await mockApi(page);
  await page.goto('/');
  await expectViewportIsPhone(page);
});

// A service worker can serve an index naming a removed bundle; the recovery is
// inline in `src/index.html`, and this checks it works.
test('a bundle a deploy removed reloads into the app, not a blank screen', async ({ page }) => {
  await mockApi(page);
  await expectRecoversFromMissingBundle(page, '/', 'text=Stop walking every transcript');
});

test('the list — long subjects beside a holder chip @ phone width', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.goto('/');
  await page.getByText('Stop walking every transcript').waitFor();
  await expectIconFontLoaded(page);
  await expectCleanLayout(page, testInfo, { allow: FILTER_SCROLLERS });
  // Holder chips ellipsise by design; clipped text elsewhere is a mistake.
});

/** The filter row scrolls sideways by design: wrapping it pushes the list below the fold. */
const FILTER_SCROLLERS = ['.filters .row'];

test('the list — filtered to one holder @ phone width', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.goto('/');
  await page.getByRole('radio', { name: 'with a session' }).click();
  await page.getByText('The console task reader').waitFor();
  await expectCleanLayout(page, testInfo, { allow: FILTER_SCROLLERS });
});

/** Code blocks and tables scroll sideways by design, named since the harness won't infer it. */
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
  // `/t/:id` declares up, so the bar leads with the arrow and names the task.
  await expectUpInTheBar(page);
  await expectCleanLayout(page, testInfo, { allow: MD_SCROLLERS });
});

test('a task — the move menu, with an unnamed session in it @ phone width', async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.goto('/t/92');
  await page.locator('.move').click();
  // The unnamed session's id has least room in this menu.
  await page.getByRole('menuitem', { name: SESSIONS[1].id }).waitFor();
  // ⚠ Scoped to the overlay: an open menu covers the page by design. Asserted: it
  // doesn't collide with itself, and a long id doesn't push it off screen.
  await expectCleanLayout(page, testInfo, { root: '.mat-mdc-menu-panel' });
  await expectNoHorizontalOverflow(page, testInfo, null, MD_SCROLLERS);
});

test('a task — the overflow menu, whose one item is a sentence @ phone width', async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.goto('/t/92');
  await page.getByRole('button', { name: 'More actions' }).click();
  // A phrase, since "drop" alone reads as "delete"; phrases are where width runs out.
  await page.getByRole('menuitem', { name: 'Drop it' }).waitFor();
  await expectCleanLayout(page, testInfo, { root: '.mat-mdc-menu-panel' });
  await expectNoHorizontalOverflow(page, testInfo, null, MD_SCROLLERS);
});

test('a dropped task — closed, and not counted as done @ phone width', async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.route('**/api/tasks/*', (r) => r.fulfill({ json: DROPPED }));
  await page.goto('/t/92');
  await page.getByText('dropped').first().waitFor();
  await expectCleanLayout(page, testInfo, { allow: MD_SCROLLERS });
});

test('filing a task — a hint under a field, and a label under that @ phone width', async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.goto('/new');
  await page.getByRole('heading', { name: 'File a task' }).waitFor();
  // Drilled into from `+`, so the bar leads with the arrow back to the list.
  await expectUpInTheBar(page);
  await page.getByLabel('Subject').fill(TASKS[1].subject);
  await expectCleanLayout(page, testInfo);
});

test('a task — closed with nothing written since it last moved @ phone width', async ({
  page,
}, testInfo) => {
  await mockApi(page);
  await page.route('**/api/tasks/*', (r) =>
    r.request().method() === 'PATCH'
      ? r.fulfill({
          json: {
            ...DETAIL,
            changed: ['status'],
            unwritten: '2026-09-20T10:00:00Z',
          } satisfies Updated,
        })
      : r.fulfill({ json: DETAIL }),
  );
  await page.goto('/t/92');
  await page
    .getByRole('radiogroup', { name: 'Status' })
    .getByRole('radio', { name: 'done' })
    .click();
  await page.getByRole('heading', { name: 'Closed with its text as it was' }).waitFor();
  await expectCleanLayout(page, testInfo, { allow: MD_SCROLLERS });
});

test('filing a task that names one you just closed @ phone width', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.route('**/api/tasks', (r) =>
    r.request().method() === 'POST'
      ? r.fulfill({
          json: {
            ...TASKS[1],
            closed: [{ id: 89, status: 'done', at: '2026-09-26T10:00:00Z' }],
          } satisfies Created,
        })
      : r.fulfill({ json: TASKS }),
  );
  await page.goto('/new');
  await page.getByLabel('Subject').fill('four more bugs found after #89');
  // The form starts on the pile here, and the pile needs a reason.
  await page.getByLabel("Why is this nobody's?").fill('any session can take it');
  await page.getByLabel('Priority').click();
  await page.getByRole('option', { name: 'P2', exact: false }).click();
  await page.getByRole('button', { name: 'File it' }).click();
  await page.getByRole('heading', { name: 'You closed what this names' }).waitFor();
  await page.getByRole('link', { name: '#89' }).waitFor();
  await expectCleanLayout(page, testInfo, { allow: MD_SCROLLERS });

  // One tap: reopen the closed task first, then drop the filing, then go there.
  const patches: string[] = [];
  page.on('request', (req) => {
    if (req.method() === 'PATCH') patches.push(`${new URL(req.url()).pathname} ${req.postData()}`);
  });
  await page.getByRole('button', { name: 'Continue in #89' }).click();
  await page.waitForURL('**/t/89');
  expect(patches).toEqual([
    '/api/tasks/89 {"status":"open"}',
    `/api/tasks/${TASKS[1].id} {"status":"dropped"}`,
  ]);
});
