import { Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import { Router } from '@angular/router';

import { reason } from './errors';
import { PRIORITIES, PRIORITY_GLOSS, sessionLabel } from './holder';
import { Assignee, Priority } from './models';
import { TaskStore } from './task-store';
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
  private store = inject(TaskStore);
  private router = inject(Router);

  readonly subject = signal('');
  readonly body = signal('');
  readonly to = signal<string>('nobody');
  /**
   * Three states, and `undefined` is the one that matters: NOT YET ANSWERED.
   *
   * ⚠ **It does not start on a value**, because a control that starts on one
   * answers for the filer. `null` here is `unassessed` — a real answer meaning
   * *nobody has judged this* — and it has to be chosen, the same way a level
   * does. Filing is blocked until one of the six is picked.
   */
  readonly priority = signal<Priority | null | undefined>(undefined);
  readonly priorities = PRIORITIES;
  readonly priorityGloss = PRIORITY_GLOSS;

  readonly sessionOptions = computed(() =>
    this.store.sessions().map((session) => ({ id: session.id, label: sessionLabel(session) })),
  );
  readonly me = computed(() => this.store.personId());
  readonly saving = signal(false);
  readonly failed = signal<string | null>(null);

  constructor() {
    this.store.ensure();
  }

  /**
   * Where the new task goes — always stated, never left out.
   *
   * ⚠ **Absence is no longer the pile.** The service now files a task to
   * whoever is filing it unless told otherwise, so returning `undefined` for
   * "nobody" would put every task the form filed onto Pippijn. The pile is a
   * choice made in the picker and it has to travel as one.
   *
   * The one case that still returns nothing is "me" before `/api/me` has
   * answered: there is no id to send, and letting the service infer the person
   * it already knows is asking is right rather than a fallback.
   */
  private assignee(): Assignee | undefined {
    const to = this.to();
    if (to === 'nobody') return { kind: 'nobody' };
    if (to === 'me') {
      const me = this.me();
      return me ? { kind: 'person', id: me } : undefined;
    }
    return { kind: 'session', id: to };
  }

  file(): void {
    const subject = this.subject().trim();
    const priority = this.priority();
    // `undefined` is unanswered; `null` is a chosen `unassessed`. The button
    // is disabled in the same state, so this is the second of two guards.
    if (!subject || priority === undefined || this.saving()) return;
    this.saving.set(true);
    this.failed.set(null);
    this.api
      .create({
        subject,
        body: this.body(),
        // SENT, always — null for unassessed. Omitting the key is refused by
        // the service now, and that is the point: it cannot be skipped.
        priority,
        assignee: this.assignee(),
      })
      .subscribe({
        next: (task) => {
          this.saving.set(false);
          // The list does not know about this one yet.
          this.store.refresh();
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
