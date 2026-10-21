import { Page, test } from '@playwright/test';

import type { Created, Task, Updated } from '../src/app/models';
import { DETAIL, DROPPED, SESSIONS, TASKS, mockApi } from './fixtures';

/**
 * Write one picture, animations finished: Material fades menus in, and
 * Playwright would shoot them half-transparent.
 */
async function shot(page: Page, name: string, whole = false): Promise<void> {
  const path = `ui-snapshots/${name}.png`;
  const viewport = page.viewportSize();
  if (!whole || !viewport) {
    await page.screenshot({ path, animations: 'disabled' });
    return;
  }
  // ⚠ Not `fullPage: true`: `main` scrolls inside a fixed shell, so a full-page
  // shot cuts the scrolled part. The viewport grows to the content instead.
  const height = await page.evaluate(() => {
    const main = document.querySelector('main');
    if (!main) return document.documentElement.scrollHeight;
    return main.scrollHeight + (window.innerHeight - main.clientHeight);
  });
  // Capped, or the image is too tall to look at.
  await page.setViewportSize({ width: viewport.width, height: Math.min(height, 4000) });
  await page.screenshot({ path, animations: 'disabled' });
  await page.setViewportSize(viewport);
}

/**
 * Render every screen to `ui-snapshots/` to be looked at (`pnpm run shots`).
 * Not a gate: geometry checks pass a truncated chip or a hint over a border.
 */
test('every screen, at phone width', async ({ page }) => {
  await mockApi(page);

  await page.goto('/');
  await page.getByText('Stop walking every transcript').waitFor();
  await shot(page, 'list', true);

  await page.goto(`/t/${TASKS[1].id}`);
  // By role, not by text: every Material icon contributes its ligature to the
  // accessible tree, so a text query for a common word matches icons too.
  await page.getByRole('heading', { name: 'History' }).waitFor();
  await shot(page, 'task', true);

  // The undo panel: a whole body as text with its buttons below, so phone height
  // is where it breaks.
  await page.getByRole('button', { name: /what the last edit replaced/ }).click();
  await page.getByText('Before the last edit').waitFor();
  await shot(page, 'undo', true);
  // ⚠ The viewport, scrolled where the tap left it: the top of the page shows none of the panel.
  await page.locator('.previous').scrollIntoViewIfNeeded();
  await shot(page, 'undo-viewport');

  // The app bar's menu.
  await page.goto('/');
  await page.getByRole('button', { name: 'Menu' }).click();
  await page.getByRole('menuitem', { name: 'Who has what' }).waitFor();
  await shot(page, 'nav');

  await page.goto(`/t/${TASKS[1].id}`);
  // The move menu, where a 36-character id has least room.
  await page.locator('.move').click();
  await page.getByRole('menuitem', { name: SESSIONS[1].id }).waitFor();
  await shot(page, 'move');

  // The rank menu: its glosses are the longest prose in the app.
  await page.goto(`/t/${TASKS[1].id}`);
  await page.getByRole('button', { name: /P4|unranked/ }).click();
  await page.getByRole('menuitem', { name: /P0/ }).waitFor();
  await shot(page, 'rank');

  // Both ways out of a task. ⚠ Reloaded, not Escape: the menu backdrop swallows the next click.
  await page.goto(`/t/${TASKS[1].id}`);
  await page.getByRole('button', { name: 'More actions' }).click();
  await page.locator('.mat-mdc-menu-panel').waitFor();
  await page.getByRole('menuitem', { name: 'Drop it' }).waitFor();
  await shot(page, 'drop');

  // The two lifecycle notes, where they land: under the body, scrolled to.
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
  await page.goto(`/t/${TASKS[1].id}`);
  await page
    .getByRole('radiogroup', { name: 'Status' })
    .getByRole('radio', { name: 'done' })
    .click();
  await page
    .getByRole('heading', { name: 'Closed with its text as it was' })
    .scrollIntoViewIfNeeded();
  await shot(page, 'closed-unwritten');

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
  await page.getByLabel("Why is this nobody's?").fill('any session can take it');
  await page.getByLabel('Priority').click();
  await page.getByRole('option', { name: 'P2', exact: false }).click();
  await page.getByRole('button', { name: 'File it' }).click();
  await page.getByRole('heading', { name: 'You closed what this names' }).scrollIntoViewIfNeeded();
  await shot(page, 'filed-continues');

  await page.route('**/api/tasks/*', (r) => r.fulfill({ json: DROPPED }));
  await page.goto(`/t/${TASKS[1].id}`);
  // By role, not by text: every Material icon contributes its ligature to the
  // accessible tree, so a text query for a common word matches icons too.
  await page.getByRole('heading', { name: 'History' }).waitFor();
  await shot(page, 'dropped', true);

  await page.goto('/who');
  await page.getByRole('heading', { name: 'Who has what' }).waitFor();
  await shot(page, 'who', true);

  // Clicking a row: the list must say which holder it shows, or empty reads as no work.
  await page.getByRole('link', { name: /memview/ }).click();
  await page.getByLabel('Showing one holder').getByText('memview').waitFor();
  await shot(page, 'who-focused', true);

  // The same for an unnamed session: a uuid chip that must keep its close icon.
  await page.goto('/who');
  await page.getByRole('link', { name: new RegExp(SESSIONS[1].id.slice(0, 8)) }).click();
  await shot(page, 'who-focused-unnamed', true);

  // Renaming the unnamed session: does the row-turned-form fit a phone?
  await page.goto('/who');
  await page.getByRole('heading', { name: 'Who has what' }).waitFor();
  await page
    .getByRole('button', { name: new RegExp(`Name ${SESSIONS[1].id.slice(0, 8)}`) })
    .click();
  await page.getByRole('textbox').waitFor();
  await shot(page, 'who-renaming', true);

  await page.goto('/new');
  await page.getByRole('heading', { name: 'File a task' }).waitFor();
  await shot(page, 'new', true);

  // The pile's reason field, which only appears once the pile is chosen.
  await page.getByRole('combobox', { name: 'For' }).click();
  await page.getByRole('option', { name: 'the pile' }).click();
  await page.getByLabel("Why is this nobody's?").waitFor();
  await shot(page, 'new-pile', true);

  // Empty: easy to leave looking broken.
  await page.route('**/api/tasks**', (r) => r.fulfill({ json: [] satisfies Task[] }));
  await page.goto('/');
  await page.getByText('Nothing here').waitFor();
  await shot(page, 'empty', true);
});
