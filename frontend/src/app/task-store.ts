import { Injectable, inject, signal } from '@angular/core';

import { reason } from './errors';
import { Holder, Me, RepoCount, Session, Task } from './models';
import { TasksApi } from './tasks-api';

/**
 * What the whole app knows, held above the views that draw it.
 *
 * **Root-provided rather than fetched per component**, which is the rule
 * `DL-ANGULAR-COMPONENT-FETCHED-LIST` states and the reason behind it: a list a
 * component fetches into its own signal is empty again the moment you navigate
 * away and back, so a phone user tapping a task and pressing back watches the
 * list blank and refill. Here it is retained, and a return is instant.
 *
 * Three things are held: the tasks, the repositories that have work, and the
 * sessions work can be handed to. All three are read by more than one screen —
 * the move menu and the file form both need the sessions — so a component-level
 * copy would also mean fetching the same thing twice on one navigation.
 *
 * ⚠ **A failure is withdrawn when the next attempt succeeds.** An error signal
 * that is only ever set stays on screen forever after one dropped request, which
 * is `DL-ANGULAR-ERROR-SIGNAL-STICKY` — and on a phone on a VPN, one dropped
 * request is a normal Tuesday.
 */
@Injectable({ providedIn: 'root' })
export class TaskStore {
  private api = inject(TasksApi);

  readonly tasks = signal<Task[]>([]);
  readonly repos = signal<RepoCount[]>([]);
  readonly sessions = signal<Session[]>([]);
  readonly holders = signal<Holder[]>([]);
  readonly me = signal<Me | null>(null);

  /** True until the first answer, of either kind. Only the first: a refresh
   *  behind an already-drawn list must not blank it. */
  readonly loading = signal(true);
  readonly failed = signal<string | null>(null);

  /** The signed-in person's id, or null when a session is driving the page. */
  personId(): string | null {
    const me = this.me();
    return me?.kind === 'person' ? me.id : null;
  }

  /** Load everything. Safe to call again — that is what a write does after it
   *  lands, so the list, the counts and the history agree. */
  refresh(): void {
    this.api.me().subscribe({
      next: (me) => this.me.set(me),
      // Not recorded as a failure: the interceptor raises the sign-in wall on
      // 401, and a second message beside it would explain nothing.
      error: () => this.me.set(null),
    });
    this.api.list().subscribe({
      next: (tasks) => {
        this.tasks.set(tasks);
        this.failed.set(null);
        this.loading.set(false);
      },
      error: (err: unknown) => {
        this.failed.set(reason(err));
        this.loading.set(false);
      },
    });
    this.api.holders().subscribe({
      next: (holders) => this.holders.set(holders),
      error: () => this.holders.set([]),
    });
    this.api.repos().subscribe({
      next: (repos) => this.repos.set(repos),
      error: () => this.repos.set([]),
    });
    this.api.sessions().subscribe({
      next: (sessions) => this.sessions.set(sessions),
      error: () => this.sessions.set([]),
    });
  }

  private loaded = false;

  /** Load once. A screen calls this on the way in; the second screen to do so
   *  gets what is already here rather than a second round trip.
   *
   *  A plain flag rather than "is the list empty": an empty list is a real
   *  answer — nothing is open — and treating it as *not loaded* would refetch on
   *  every navigation exactly when there is least to show for it. */
  ensure(): void {
    if (this.loaded) return;
    this.loaded = true;
    this.refresh();
  }
}
