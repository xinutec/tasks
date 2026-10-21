import { Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import { Router } from '@angular/router';
import { scaffoldTitle } from '@xinutec/ui-scaffold';

import { reason } from './errors';
import { PRIORITIES, PRIORITY_GLOSS, sessionLabel } from './holder';
import { Assignee, Priority } from './models';
import { TaskStore } from './task-store';
import { TasksApi } from './tasks-api';

/** File a task. The subject is capped at the column and says so: it reaches every prompt. */
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
  readonly extrasFailed = this.store.extrasFailed;
  private router = inject(Router);

  readonly subject = signal('');
  readonly body = signal('');
  readonly to = signal<string>('nobody');
  /**
   * Three states: `undefined` is not yet answered, `null` is a chosen
   * `unassessed`. No starting value, which would answer for the filer; filing is
   * blocked until one of the six is picked.
   */
  readonly priority = signal<Priority | null | undefined>(undefined);
  readonly priorities = PRIORITIES;
  readonly priorityGloss = PRIORITY_GLOSS;

  readonly sessionOptions = computed(() =>
    this.store.sessions().map((session) => ({ id: session.id, label: sessionLabel(session) })),
  );
  readonly me = computed(() => this.store.personId());
  /** Why the pile, when the pile is chosen — see the note on `file`. */
  readonly spare = signal('');
  readonly saving = signal(false);
  readonly failed = signal<string | null>(null);

  constructor() {
    scaffoldTitle(() => 'File a task');
    this.store.ensure();
  }

  /**
   * Where the new task goes, always stated: absent means the filer, not the pile.
   * Only "me" before `/api/me` answers sends nothing, and then the service
   * rightly infers the person.
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
    // Unanswered: the button is disabled too, so this is the second guard. The
    // pile needs its reason; checked here rather than via a 400.
    const spare = this.spare().trim();
    if (this.to() === 'nobody' && !spare) return;
    if (!subject || priority === undefined || this.saving()) return;
    this.saving.set(true);
    this.failed.set(null);
    this.api
      .create({
        subject,
        body: this.body(),
        // Always sent, null for unassessed: the service refuses an omitted key.
        priority,
        assignee: this.assignee(),
        // Only with the pile; refused beside a real holder.
        ...(this.to() === 'nobody' ? { spare } : {}),
        // The UI can't skip the check, and says so explicitly.
        checked: true,
      })
      .subscribe({
        next: (task) => {
          this.saving.set(false);
          // The list does not know about this one yet.
          this.store.refresh();
          // Straight to the task: the next thing is usually to add to it.
          void this.router.navigate(['/t', task.id], {
            // `task-view` shows these; only this response carries them.
            state: { filed: task.id, closed: task.closed ?? [] },
          });
        },
        // The service's message, which names the field.
        error: (err: unknown) => {
          this.saving.set(false);
          this.failed.set(reason(err));
        },
      });
  }
}
