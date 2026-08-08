import { test } from '@playwright/test';

import { SESSIONS, TASKS, mockApi } from './fixtures';

/**
 * Render every screen and write the picture out, so it can be **looked at**.
 *
 * Not a gate — it asserts nothing and is not in `gate.dhall`. It exists because
 * geometry checks are not sight: the first render of this app passed the whole
 * layout harness while shipping two defects that are obvious in a screenshot and
 * invisible to a measurement. A chip capped at 9ch turned `memview` into
 * `memv…`, failing the one thing a holder chip is for; and a two-line
 * `mat-hint` overflowed Material's one-line subscript slot onto the outline of
 * the field below — text over a *border*, which no text-overlap check will ever
 * call a collision.
 *
 * `pnpm run shots`, then open `ui-snapshots/`. Same fixtures as the assertions,
 * so the picture is of exactly what was measured.
 */
test('every screen, at phone width', async ({ page }) => {
  await mockApi(page);

  await page.goto('/');
  await page.getByText('Stop walking every transcript').waitFor();
  await page.screenshot({ path: 'ui-snapshots/list.png', fullPage: true });

  await page.goto(`/t/${TASKS[1].id}`);
  await page.getByText('History').waitFor();
  await page.screenshot({ path: 'ui-snapshots/task.png', fullPage: true });

  // The move is what this app is for, and its menu is where the longest thing
  // on the site — a 36-character session id — has the least room.
  await page.locator('.move').click();
  await page.getByRole('menuitem', { name: SESSIONS[1].id }).waitFor();
  await page.screenshot({ path: 'ui-snapshots/move.png' });

  await page.goto('/new');
  await page.getByRole('heading', { name: 'File a task' }).waitFor();
  await page.screenshot({ path: 'ui-snapshots/new.png', fullPage: true });

  // Empty is a state somebody sees on the first day and after the last task is
  // finished, and an empty screen is the easiest one to leave looking broken.
  await page.route('**/api/tasks**', (r) => r.fulfill({ json: [] }));
  await page.goto('/');
  await page.getByText('Nothing here').waitFor();
  await page.screenshot({ path: 'ui-snapshots/empty.png', fullPage: true });
});
