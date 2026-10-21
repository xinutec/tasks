import { Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { RouterLink } from '@angular/router';
import { scaffoldTitle } from '@xinutec/ui-scaffold';

import { reason } from './errors';
import { focusOn, said, whoParam } from './holder';
import { TaskStore } from './task-store';
import { TasksApi } from './tasks-api';

/**
 * Who has what, as `open/total`: the total is why this screen exists (`0` and
 * `0/56` are idle and cleared). The backend's order, so it matches
 * `task sessions`.
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
  readonly extrasFailed = this.store.extrasFailed;
  private api = inject(TasksApi);

  readonly loading = this.store.loading;

  /** The session being renamed; one at a time, so typing can't be lost to a second field. */
  readonly renaming = signal<string | null>(null);
  readonly draft = signal('');
  readonly saving = signal(false);
  readonly failed = signal<string | null>(null);

  readonly rows = computed(() =>
    this.store.holders().map((holder) => ({
      ...holder,
      // An unnamed session shows its id: a blank reads as a bug.
      label: said(holder.name) ?? said(holder.id) ?? 'unnamed',
      // The id, for `task move`, only beside a name.
      handle: holder.kind === 'session' && said(holder.name) ? (holder.id ?? null) : null,
      done: holder.total - holder.open,
      focus: whoParam(focusOn({ kind: holder.kind, id: holder.id })),
    })),
  );

  readonly total = computed(() => this.rows().reduce((sum, row) => sum + row.total, 0));

  constructor() {
    scaffoldTitle(() => 'Who has what');
    this.store.ensure();
  }

  /**
   * Start renaming, seeded with the current name (a rename usually corrects a
   * word); an unnamed session starts empty, since its label is a uuid.
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
   * Save and reload: names are resolved through joins everywhere, so every chip
   * is stale. A blank name is refused, so the button is disabled for one.
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
