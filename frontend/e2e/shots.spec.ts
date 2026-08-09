import { Page, test } from '@playwright/test';

import { DROPPED, SESSIONS, TASKS, mockApi } from './fixtures';

/**
 * Write one picture out, with the animations finished.
 *
 * ⚠ **`animations: 'disabled'` is not a nicety here — without it these were
 * pictures of a transition.** Playwright fires the screenshot as soon as the
 * element is visible, and Material fades a menu in over ~120 ms, so the move
 * menu was captured at part opacity — legible enough to look deliberate, which
 * is the worst way for a reference image to be wrong — and the overflow menu
 * was captured at nearly zero and came out as a page with no menu on it. The
 * option fast-forwards every CSS animation and transition to its end state
 * first, which is what a person opening the app actually sees.
 */
async function shot(page: Page, name: string, whole = false): Promise<void> {
  const path = `ui-snapshots/${name}.png`;
  const viewport = page.viewportSize();
  if (!whole || !viewport) {
    await page.screenshot({ path, animations: 'disabled' });
    return;
  }
  // ⚠ **`fullPage: true` was a lie on every screen of this app**, and silently:
  // it grows the capture to the *document's* scroll height, and this layout
  // never scrolls the document — `main` scrolls inside a fixed shell, so the
  // whole history of a task sat 78 px below a picture that claimed to be the
  // whole page. So the viewport is grown to the content instead, which is the
  // only thing that makes the scrolled part render at all.
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
  await shot(page, 'list', true);

  await page.goto(`/t/${TASKS[1].id}`);
  await page.getByText('History').waitFor();
  await shot(page, 'task', true);

  // The move is what this app is for, and its menu is where the longest thing
  // on the site — a 36-character session id — has the least room.
  await page.locator('.move').click();
  await page.getByRole('menuitem', { name: SESSIONS[1].id }).waitFor();
  await shot(page, 'move');

  // Both ways out of a task: the overflow menu that offers the second one, and
  // what a task looks like once it has been taken. ⚠ Reloaded rather than
  // pressing Escape — dismissing the move menu leaves its backdrop up through
  // the close animation, and the next click lands on that instead of on the
  // button, which produced a "menu" screenshot with no menu in it.
  await page.goto(`/t/${TASKS[1].id}`);
  await page.getByRole('button', { name: 'More actions' }).click();
  await page.locator('.mat-mdc-menu-panel').waitFor();
  await page.getByRole('menuitem', { name: 'Drop it' }).waitFor();
  await shot(page, 'drop');

  await page.route('**/api/tasks/*', (r) => r.fulfill({ json: DROPPED }));
  await page.goto(`/t/${TASKS[1].id}`);
  await page.getByText('History').waitFor();
  await shot(page, 'dropped', true);

  await page.goto('/who');
  await page.getByRole('heading', { name: 'Who has what' }).waitFor();
  await shot(page, 'who', true);

  // #657: the row is the link, and the list it reaches has to SAY which holder
  // it is showing — none of the four buckets is lit on arrival, so without the
  // holder's own chip the filter is invisible and an empty result reads as no
  // work existing. Captured by clicking rather than by visiting the URL, so the
  // picture is evidence the link exists and points where it says.
  await page.getByRole('link', { name: /memview/ }).click();
  await page.getByRole('button', { name: /memview/ }).waitFor();
  await shot(page, 'who-focused', true);

  // The same screen for a session that never named itself: 36 characters of
  // uuid in a chip that also has to keep its close icon reachable.
  await page.goto('/who');
  await page.getByRole('link', { name: new RegExp(SESSIONS[1].id.slice(0, 8)) }).click();
  await shot(page, 'who-focused-unnamed', true);

  await page.goto('/new');
  await page.getByRole('heading', { name: 'File a task' }).waitFor();
  await shot(page, 'new', true);

  // Empty is a state somebody sees on the first day and after the last task is
  // finished, and an empty screen is the easiest one to leave looking broken.
  await page.route('**/api/tasks**', (r) => r.fulfill({ json: [] }));
  await page.goto('/');
  await page.getByText('Nothing here').waitFor();
  await shot(page, 'empty', true);
});
