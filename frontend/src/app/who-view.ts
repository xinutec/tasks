import { Component, computed, inject } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { RouterLink } from '@angular/router';

import { focusOn, said, whoParam } from './holder';
import { TaskStore } from './task-store';

/**
 * Who has what: every session, Pippijn, and the pile, as `open/total`.
 *
 * ⚠ **The second number is the reason this screen exists.** The list already
 * shows who is holding each open task, so `open` alone was already visible one
 * row at a time; what was not visible anywhere is who has *finished* anything,
 * because a task leaves every open list the moment it is done. `0` and `0/56`
 * are the difference between a session with nothing to do and one that has
 * cleared a plate.
 *
 * Ordered by what is open, most first — the question this answers is who is
 * loaded — with Pippijn and the pile last regardless, since they are landmarks
 * rather than entries in the ranking. Both orderings are the backend's, so the
 * app and `task sessions` cannot disagree about them.
 */
@Component({
  selector: 'app-who-view',
  templateUrl: './who-view.html',
  styleUrl: './who-view.scss',
  imports: [RouterLink, MatIconModule, MatProgressBarModule],
})
export class WhoView {
  private store = inject(TaskStore);

  readonly loading = this.store.loading;

  readonly rows = computed(() =>
    this.store.holders().map((holder) => ({
      ...holder,
      // A session that has never named itself shows its id, because a blank
      // where a name goes reads as a bug rather than as an unnamed session.
      label: said(holder.name) ?? said(holder.id) ?? 'unnamed',
      // Only for a session, and only when it is also showing a name: it is the
      // handle for `task move`, and it is 36 characters of uuid otherwise.
      handle: holder.kind === 'session' && said(holder.name) ? (holder.id ?? null) : null,
      done: holder.total - holder.open,
      // Where the row goes. The counts were the end of the road until #657.
      focus: whoParam(focusOn({ kind: holder.kind, id: holder.id })),
    })),
  );

  /** Everything ever filed, so a row can be read against the whole. */
  readonly total = computed(() => this.rows().reduce((sum, row) => sum + row.total, 0));

  constructor() {
    this.store.ensure();
  }
}
