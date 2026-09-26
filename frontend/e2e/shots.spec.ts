import { Page, test } from '@playwright/test';

import { DETAIL, DROPPED, SESSIONS, TASKS, mockApi } from './fixtures';

/**
 * Write one picture out, with the animations finished.
 *
 * ⚠ **`animations: 'disabled'` is not a nicety**: Playwright shoots as soon as
 * the element is visible, and Material fades a menu in, so without it a menu is
 * captured part-transparent — legible enough to look deliberate — or not at
 * all.
 */
async function shot(page: Page, name: string, whole = false): Promise<void> {
  const path = `ui-snapshots/${name}.png`;
  const viewport = page.viewportSize();
  if (!whole || !viewport) {
    await page.screenshot({ path, animations: 'disabled' });
    return;
  }
  // ⚠ **Not `fullPage: true`**, which grows the capture to the *document's*
  // height: this layout scrolls `main` inside a fixed shell, so a full-page
  // shot silently cuts the scrolled part off. The viewport is grown to the
  // content instead.
  const height = await page.evaluate(() => {
    const main = document.querySelector('main');
    if (!main) return document.documentElement.scrollHeight;
    return main.scrollHeight + (window.innerHeight - main.clientHeight);
  });
  // Capped: a list of every open task would otherwise produce an image nobody
  // can look at, which is the same failure as one that is cut off.
  await page.setViewportSize({ width: viewport.width, height: Math.min(height, 4000) });
  await page.screenshot({ path, animations: 'disabled' });
  await page.setViewportSize(viewport);
}

/**
 * Render every screen and write the picture out, so it can be **looked at**.
 *
 * Not a gate — it asserts nothing and is not in `gate.dhall`. Geometry checks
 * are not sight: a truncated chip label, or a hint drawn over a field's border
 * rather than over text, passes every measurement.
 *
 * `pnpm run shots`, then open `ui-snapshots/`. Same fixtures as the assertions,
 * so the picture is of exactly what was measured.
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

  // The undo panel, which is the one place the app shows a whole stored body as
  // plain text. It is the tallest thing on the site and its two buttons sit
  // UNDER it, so phone height is exactly where it can go wrong: a confirmation
  // you have to scroll past is one nobody reads.
  await page.getByRole('button', { name: /what the last edit replaced/ }).click();
  await page.getByText('Before the last edit').waitFor();
  await shot(page, 'undo', true);
  // And the viewport alone, scrolled where the tap left it. ⚠ Scrolled
  // deliberately: the panel opens below a whole task body, so a capture at the
  // top of the page shows none of it and says nothing about whether the two
  // buttons are reachable — which is the only question this shot is for.
  await page.locator('.previous').scrollIntoViewIfNeeded();
  await shot(page, 'undo-viewport');

  // The app bar's own menu, beside the task screen's differently-glyphed one.
  await page.goto('/');
  await page.getByRole('button', { name: 'Menu' }).click();
  await page.getByRole('menuitem', { name: 'Who has what' }).waitFor();
  await shot(page, 'nav');

  await page.goto(`/t/${TASKS[1].id}`);
  // The move is what this app is for, and its menu is where the longest thing
  // on the site — a 36-character session id — has the least room.
  await page.locator('.move').click();
  await page.getByRole('menuitem', { name: SESSIONS[1].id }).waitFor();
  await shot(page, 'move');

  // The rank menu, for the same reason as the move above: every item carries a
  // one-line gloss, and those lines are the longest prose the app renders. If
  // they are going to overflow or wrap badly it is at phone width, in a menu.
  await page.goto(`/t/${TASKS[1].id}`);
  await page.getByRole('button', { name: /P4|unranked/ }).click();
  await page.getByRole('menuitem', { name: /P0/ }).waitFor();
  await shot(page, 'rank');

  // Both ways out of a task: the overflow menu that offers the second one, and
  // what a task looks like once it has been taken. ⚠ Reloaded rather than
  // pressing Escape: the move menu's backdrop stays up through the close
  // animation and takes the next click.
  await page.goto(`/t/${TASKS[1].id}`);
  await page.getByRole('button', { name: 'More actions' }).click();
  await page.locator('.mat-mdc-menu-panel').waitFor();
  await page.getByRole('menuitem', { name: 'Drop it' }).waitFor();
  await shot(page, 'drop');

  // The two lifecycle notes, where they land: under the body, scrolled to.
  await page.route('**/api/tasks/*', (r) =>
    r.request().method() === 'PATCH'
      ? r.fulfill({ json: { ...DETAIL, changed: ['status'], unwritten: '2026-09-20T10:00:00Z' } })
      : r.fulfill({ json: DETAIL }),
  );
  await page.goto(`/t/${TASKS[1].id}`);
  await page.getByRole('group', { name: 'Status' }).getByRole('button', { name: 'done' }).click();
  await page
    .getByRole('heading', { name: 'Closed with its text as it was' })
    .scrollIntoViewIfNeeded();
  await shot(page, 'closed-unwritten');

  await page.route('**/api/tasks', (r) =>
    r.request().method() === 'POST'
      ? r.fulfill({
          json: { ...TASKS[1], closed: [{ id: 89, status: 'done', at: '2026-09-26T10:00:00Z' }] },
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

  // The row is the link, and the list it reaches has to SAY which holder it is
  // showing, or an empty result reads as no work existing. Captured by clicking
  // rather than visiting the URL, so the picture shows the link works.
  await page.getByRole('link', { name: /memview/ }).click();
  await page.getByRole('button', { name: /memview/ }).waitFor();
  await shot(page, 'who-focused', true);

  // The same screen for a session that never named itself: 36 characters of
  // uuid in a chip that also has to keep its close icon reachable.
  await page.goto('/who');
  await page.getByRole('link', { name: new RegExp(SESSIONS[1].id.slice(0, 8)) }).click();
  await shot(page, 'who-focused-unnamed', true);

  // Naming a conversation, on the row that most needs it: the unnamed session,
  // whose label is 36 characters of uuid. The row BECOMES the form, so this is
  // the picture that says whether a field, a cancel and a save fit across a
  // phone beside nothing else.
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

  // The pile's reason field only exists once the pile is chosen, and a control
  // that appears on a choice is the one that gets left broken — nothing renders
  // it in the default state above.
  await page.getByRole('combobox', { name: 'For' }).click();
  await page.getByRole('option', { name: 'the pile' }).click();
  await page.getByLabel("Why is this nobody's?").waitFor();
  await shot(page, 'new-pile', true);

  // Empty is a state somebody sees on the first day and after the last task is
  // finished, and an empty screen is the easiest one to leave looking broken.
  await page.route('**/api/tasks**', (r) => r.fulfill({ json: [] }));
  await page.goto('/');
  await page.getByText('Nothing here').waitFor();
  await shot(page, 'empty', true);
});
