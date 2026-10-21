import { DatePipe } from '@angular/common';
import { Component, computed, effect, inject, input, signal } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatButtonToggleModule } from '@angular/material/button-toggle';
import { MatCardModule } from '@angular/material/card';
import { MatChipsModule } from '@angular/material/chips';
import { MatIconModule } from '@angular/material/icon';
import { MatMenuModule } from '@angular/material/menu';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { Router, RouterLink } from '@angular/router';
import { scaffoldTitle } from '@xinutec/ui-scaffold';
import { switchMap } from 'rxjs';

import { reason } from './errors';
import {
  PRIORITIES,
  PRIORITY_GLOSS,
  STATUS_ICON,
  STATUS_LABEL,
  holderLabel,
  sessionLabel,
} from './holder';
import { Assignee, Closed, Priority, Revision, Status, TaskDetail } from './models';
import { TaskStore } from './task-store';
import { TasksApi } from './tasks-api';

/**
 * One task: what it is, who has it, and its history. The move control is why
 * the screen exists, so it is one tap from the top, not behind an edit mode.
 */
@Component({
  selector: 'app-task-view',
  templateUrl: './task-view.html',
  styleUrl: './task-view.scss',
  imports: [
    DatePipe,
    MatButtonModule,
    MatButtonToggleModule,
    MatCardModule,
    MatChipsModule,
    MatIconModule,
    MatMenuModule,
    MatProgressBarModule,
    RouterLink,
  ],
})
export class TaskView {
  /** From the route (`withComponentInputBinding`), so the URL says which task is open. */
  readonly id = input.required<string>();

  private api = inject(TasksApi);
  private router = inject(Router);
  private store = inject(TaskStore);
  readonly extrasFailed = this.store.extrasFailed;

  readonly statusIcon = STATUS_ICON;
  readonly statusLabel = STATUS_LABEL;
  /** The progression as buttons; `dropped` is the other way out, in the overflow menu. */
  readonly statuses: Status[] = ['open', 'doing', 'done'];
  readonly priorities = PRIORITIES;
  readonly priorityGloss = PRIORITY_GLOSS;

  readonly task = signal<TaskDetail | null>(null);
  /** Labelled here, not by a template method call, which runs every change-detection pass. */
  readonly sessionOptions = computed(() =>
    this.store.sessions().map((session) => ({ id: session.id, label: sessionLabel(session) })),
  );
  readonly loading = signal(true);
  readonly failed = signal<string | null>(null);
  /** A write in flight: controls go inert rather than hiding, so the page doesn't jump. */
  readonly saving = signal(false);
  /** The signed-in person's id, so "give it to me" names them. */
  readonly me = computed(() => this.store.personId());

  /**
   * The version the last edit replaced, fetched on a tap, never on load: it is a
   * whole second body, and `TaskDetail.restorable` decides whether to offer it.
   */
  readonly previous = signal<Revision | null>(null);
  readonly peeking = signal(false);

  /**
   * Closed tasks the filing names that its filer just closed (`tasks::lifecycle`),
   * passed in navigation state, keyed by task so they don't follow a link away.
   */
  readonly continues = signal<{ task: number; closed: Closed[] } | null>(null);

  /** When a close here found the text unrewritten, keyed like `continues`. */
  readonly unwritten = signal<{ task: number; at: string } | null>(null);

  constructor() {
    // The bar shows the number; the subject is the headline.
    scaffoldTitle(() => `#${this.id()}`);
    const handed = this.router.currentNavigation()?.extras.state as
      { filed?: number; closed?: Closed[] } | undefined;
    if (handed?.filed !== undefined && handed.closed?.length) {
      this.continues.set({ task: handed.filed, closed: handed.closed });
    }
    // An effect, not `ngOnInit`: the router reuses this component across `#4` → `#7`.
    effect(() => {
      // Cleared with the task, or #7 would open showing #4's previous version.
      this.previous.set(null);
      this.load(Number(this.id()));
    });
    this.store.ensure();
  }

  /**
   * Show what putting it back would restore. Two taps, the first only reading:
   * undo overwrites the page, and the content answers what a reader asks better
   * than "are you sure?".
   */
  peek(): void {
    const task = this.task();
    if (!task || this.peeking()) return;
    this.peeking.set(true);
    this.api.previous(task.id).subscribe({
      next: (was) => {
        this.previous.set(was);
        this.peeking.set(false);
      },
      error: (err: unknown) => {
        this.peeking.set(false);
        this.failed.set(reason(err));
      },
    });
  }

  /** Put both columns back, as an ordinary, undoable edit. */
  restore(): void {
    const was = this.previous();
    if (!was) return;
    this.previous.set(null);
    this.change({ subject: was.subject, body: was.body, replace_body: true });
  }

  /** Close the panel without restoring. */
  dismiss(): void {
    this.previous.set(null);
  }

  /** Hand it to the person; hidden when a session drives the page. */
  moveToMe(): void {
    const me = this.me();
    if (me) this.moveTo({ kind: 'person', id: me });
  }

  private load(id = Number(this.id())): void {
    this.api.task(id).subscribe({
      next: (task) => {
        this.task.set(task);
        // Withdrawn on success: an error signal that is only ever set stays on
        // screen after the retry that fixed it (`DL-ANGULAR-ERROR-SIGNAL-STICKY`).
        this.failed.set(null);
        this.loading.set(false);
      },
      error: (err: unknown) => {
        this.failed.set(reason(err));
        this.loading.set(false);
      },
    });
  }

  holder(assignee: Assignee): string {
    return holderLabel(assignee);
  }

  setStatus(status: Status): void {
    this.change({ status });
  }

  /** No "unranked" item, as in the CLI: rank again to correct. */
  setPriority(priority: Priority): void {
    this.change({ priority });
  }

  /** Close it without doing it; tapping `open` undoes it. */
  drop(): void {
    this.change({ status: 'dropped' });
  }

  /**
   * The remedy for a filing that continues a closed task, in one tap: reopen that,
   * drop this, go there. Reopen first, so a failure drops nothing.
   */
  continueIn(closed: number): void {
    const task = this.task();
    if (!task || this.saving()) return;
    this.saving.set(true);
    this.api
      .change(closed, { status: 'open' })
      .pipe(switchMap(() => this.api.change(task.id, { status: 'dropped' })))
      .subscribe({
        next: () => {
          this.saving.set(false);
          this.continues.set(null);
          this.store.refresh();
          void this.router.navigate(['/t', closed]);
        },
        error: (err: unknown) => {
          this.saving.set(false);
          this.failed.set(reason(err));
        },
      });
  }

  moveTo(assignee: Assignee): void {
    this.change({ assignee });
  }

  private change(change: Parameters<TasksApi['change']>[1]): void {
    const task = this.task();
    if (!task || this.saving()) return;
    this.saving.set(true);
    this.api.change(task.id, change).subscribe({
      next: (updated) => {
        this.saving.set(false);
        this.unwritten.set(updated.unwritten ? { task: task.id, at: updated.unwritten } : null);
        // Re-read rather than patched: the write also appended to the history, which
        // must not disagree with the status. `load` clears the failure.
        this.load();
        // The list is stale too.
        this.store.refresh();
      },
      error: (err: unknown) => {
        this.saving.set(false);
        this.failed.set(reason(err));
      },
    });
  }
}
