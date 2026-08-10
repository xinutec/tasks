import { Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { RouterLink } from '@angular/router';

import { reason } from './errors';
import { focusOn, said, whoParam } from './holder';
import { TaskStore } from './task-store';
import { TasksApi } from './tasks-api';

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
  imports: [
    RouterLink,
    FormsModule,
    MatButtonModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatProgressBarModule,
  ],
})
export class WhoView {
  private store = inject(TaskStore);
  private api = inject(TasksApi);

  readonly loading = this.store.loading;

  /** The session id being renamed, or null. One at a time: two open fields on a
   *  phone is two ways to lose what you typed. */
  readonly renaming = signal<string | null>(null);
  readonly draft = signal('');
  readonly saving = signal(false);
  readonly failed = signal<string | null>(null);

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

  /**
   * Start naming a conversation.
   *
   * ⚠ **Seeded with the CURRENT name, not blank.** A rename is usually a
   * correction of a word, and clearing the field first is the shape that loses
   * one. An unnamed session seeds empty, because its label is a uuid and nobody
   * is editing that.
   */
  startRename(id: string, current: string | null): void {
    this.failed.set(null);
    this.draft.set(current ?? '');
    this.renaming.set(id);
  }

  cancelRename(): void {
    this.renaming.set(null);
    this.failed.set(null);
  }

  /**
   * Save it, and reload.
   *
   * The refresh is not decoration: a name is resolved through the join wherever
   * it appears, so the list's holder chips and every `from` on a pile row are
   * stale the moment this lands. Blank is refused by the service — it is a
   * write that would otherwise report success and keep the old name — so the
   * button is disabled rather than letting somebody find that out.
   */
  saveRename(id: string): void {
    const name = this.draft().trim();
    if (!name || this.saving()) return;
    this.saving.set(true);
    this.failed.set(null);
    this.api.rename(id, name).subscribe({
      next: () => {
        this.saving.set(false);
        this.renaming.set(null);
        this.store.refresh();
      },
      error: (err: unknown) => {
        this.saving.set(false);
        this.failed.set(reason(err));
      },
    });
  }
}
