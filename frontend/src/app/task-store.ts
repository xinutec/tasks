import { Injectable, inject, signal } from '@angular/core';

import { reason } from './errors';
import { Holder, Me, Session, Task } from './models';
import { TasksApi } from './tasks-api';

/**
 * What the whole app knows, held above the views: root-provided, so going back
 * is instant rather than blank-and-refill (`DL-ANGULAR-COMPONENT-FETCHED-LIST`).
 * A failure is withdrawn by the next success (`DL-ANGULAR-ERROR-SIGNAL-STICKY`).
 */
@Injectable({ providedIn: 'root' })
export class TaskStore {
  private api = inject(TasksApi);

  readonly tasks = signal<Task[]>([]);
  readonly sessions = signal<Session[]>([]);
  readonly holders = signal<Holder[]>([]);
  readonly me = signal<Me | null>(null);

  /** True until the first answer only: a refresh must not blank a drawn list. */
  readonly loading = signal(true);
  readonly failed = signal<string | null>(null);
  /** Holders or sessions unreadable: unknown, not empty. */
  readonly extrasFailed = signal<string | null>(null);

  /** The signed-in person's id, or null when a session is driving the page. */
  personId(): string | null {
    const me = this.me();
    return me?.kind === 'person' ? me.id : null;
  }

  /** Load everything; writes call it again so list, counts and history agree. */
  refresh(): void {
    this.extrasFailed.set(null);
    this.api.me().subscribe({
      next: (me) => this.me.set(me),
      // Not a failure: the interceptor shows the sign-in wall on 401.
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
      error: (err: unknown) => this.extrasFailed.set(reason(err)),
    });
    this.api.sessions().subscribe({
      next: (sessions) => this.sessions.set(sessions),
      error: (err: unknown) => this.extrasFailed.set(reason(err)),
    });
  }

  private loaded = false;

  /**
   * Load once per app: a flag, not "is it empty", since an empty list is a real
   * answer.
   */
  ensure(): void {
    if (this.loaded) return;
    this.loaded = true;
    this.refresh();
  }
}
