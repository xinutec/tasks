import { Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import { Router } from '@angular/router';

import { reason } from './errors';
import { sessionLabel } from './holder';
import { Assignee, RepoCount, Session } from './models';
import { TasksApi } from './tasks-api';

/**
 * File a task.
 *
 * ⚠ **The subject field is capped at the length the column is, and says so.**
 * That cap is not tidiness: the subject is the only part of a task that ever
 * reaches a prompt, and it reaches one on every turn for as long as the task is
 * open. The field is where that is cheapest to explain, because it is the moment
 * somebody is about to write an essay into it.
 */
@Component({
  selector: 'app-new-view',
  templateUrl: './new-view.html',
  styleUrl: './new-view.scss',
  imports: [
    FormsModule,
    MatButtonModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatSelectModule,
  ],
})
export class NewView {
  /** Matches `MAX_SUBJECT` in `tasks::types`, and the column. */
  readonly maxSubject = 200;

  private api = inject(TasksApi);
  private router = inject(Router);

  readonly subject = signal('');
  readonly body = signal('');
  readonly repo = signal<string>('');
  readonly to = signal<string>('nobody');

  readonly repos = signal<RepoCount[]>([]);
  readonly sessions = signal<Session[]>([]);
  readonly sessionOptions = computed(() =>
    this.sessions().map((session) => ({ id: session.id, label: sessionLabel(session) })),
  );
  readonly me = signal<string | null>(null);
  readonly saving = signal(false);
  readonly failed = signal<string | null>(null);

  constructor() {
    this.api.repos().subscribe({
      next: (repos) => this.repos.set(repos),
      error: () => this.repos.set([]),
    });
    this.api.sessions().subscribe({
      next: (sessions) => this.sessions.set(sessions),
      error: () => this.sessions.set([]),
    });
    this.api.me().subscribe({
      next: (me) => this.me.set(me.kind === 'person' ? me.id : null),
      error: () => this.me.set(null),
    });
  }

  private assignee(): Assignee | undefined {
    const to = this.to();
    if (to === 'nobody') return undefined;
    if (to === 'me') {
      const me = this.me();
      return me ? { kind: 'person', id: me } : undefined;
    }
    return { kind: 'session', id: to };
  }

  file(): void {
    const subject = this.subject().trim();
    if (!subject || this.saving()) return;
    this.saving.set(true);
    this.failed.set(null);
    this.api
      .create({
        subject,
        body: this.body(),
        repo: this.repo() || undefined,
        assignee: this.assignee(),
      })
      .subscribe({
        next: (task) => {
          this.saving.set(false);
          // Straight to the task rather than back to the list: the next thing
          // wanted after filing one is usually to add to it.
          void this.router.navigate(['/t', task.id]);
        },
        // The service's own message, which says which field was wrong — a
        // generic "could not save" here would send somebody guessing.
        error: (err: unknown) => {
          this.saving.set(false);
          this.failed.set(reason(err));
        },
      });
  }
}
