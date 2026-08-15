import { DatePipe } from '@angular/common';
import { Component, computed, effect, inject, input, signal } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatMenuModule } from '@angular/material/menu';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { RouterLink } from '@angular/router';

import { reason } from './errors';
import {
  PRIORITIES,
  PRIORITY_GLOSS,
  STATUS_ICON,
  STATUS_LABEL,
  holderLabel,
  sessionLabel,
} from './holder';
import { Assignee, Priority, Revision, Status, TaskDetail } from './models';
import { TaskStore } from './task-store';
import { TasksApi } from './tasks-api';

/**
 * One task: what it is, who has it, and everything that has happened to it.
 *
 * **The move control is the reason this screen exists.** Everything else here
 * could be a row in a list; handing a task to a particular session, or taking
 * one back, is the act the whole service was built around, so it is one tap from
 * the top of the page and not behind an edit mode.
 */
@Component({
  selector: 'app-task-view',
  templateUrl: './task-view.html',
  styleUrl: './task-view.scss',
  imports: [
    DatePipe,
    MatButtonModule,
    MatIconModule,
    MatMenuModule,
    MatProgressBarModule,
    RouterLink,
  ],
})
export class TaskView {
  /** Bound from the route (`withComponentInputBinding`), so the URL is what
   *  says which task is open. */
  readonly id = input.required<string>();

  private api = inject(TasksApi);
  private store = inject(TaskStore);

  readonly statusIcon = STATUS_ICON;
  readonly statusLabel = STATUS_LABEL;
  /** The progression, as buttons. `dropped` is deliberately not in it — it is
   *  the other way out rather than the next step along, and it lives in the
   *  overflow menu. */
  readonly statuses: Status[] = ['open', 'doing', 'done'];
  readonly priorities = PRIORITIES;
  readonly priorityGloss = PRIORITY_GLOSS;

  readonly task = signal<TaskDetail | null>(null);
  /** The move menu's destinations, labelled here rather than in the template:
   *  a method call in a template runs on every change-detection pass, and
   *  `DL-ANGULAR-TEMPLATE-METHOD-CALL` exists because that is invisible. */
  readonly sessionOptions = computed(() =>
    this.store.sessions().map((session) => ({ id: session.id, label: sessionLabel(session) })),
  );
  readonly loading = signal(true);
  readonly failed = signal<string | null>(null);
  /** A write in flight. The controls stay visible and go inert: hiding them
   *  would make the page jump under the thumb that just tapped one. */
  readonly saving = signal(false);
  /** The signed-in person's own id, so "give it to me" names them rather than
   *  a username written into a template. */
  readonly me = computed(() => this.store.personId());

  /**
   * The version this task's last edit replaced, once it has been asked for.
   *
   * ⚠ **Fetched on a tap, never on load.** It is a whole second body, and the
   * reader who wants it is rare; `TaskDetail.restorable` is the cheap boolean
   * that decides whether to offer the tap at all.
   */
  readonly previous = signal<Revision | null>(null);
  readonly peeking = signal(false);

  constructor() {
    // An effect rather than `ngOnInit`, because the router reuses this
    // component when only the parameter changes: going from #4 to #7 through a
    // link would otherwise leave #4 on the screen with #7 in the address bar.
    effect(() => {
      // Cleared with the task, or #7 would open showing #4's previous version.
      this.previous.set(null);
      this.load(Number(this.id()));
    });
    this.store.ensure();
  }

  /**
   * Show what putting it back would put back.
   *
   * ⚠ **Two taps, and the first one only reads.** Undo overwrites the text
   * currently on the page, so an accidental brush against a one-tap control
   * would be the same class of accident this whole feature exists to repair —
   * on a phone, where the thumb is imprecise, most of all. Showing the content
   * rather than asking "are you sure?" also answers the question a reader
   * actually has, which is *what would come back*.
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

  /** Put both columns back. Itself an ordinary edit, so it is undoable in turn
   *  and the history records that it happened. */
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

  /** Hand it to the person. Hidden when a session is driving the page, which
   *  has no "me" to hand it to — it uses the CLI. */
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

  /** ⚠ **No "unranked" item, matching the CLI.** Absence means *leave it alone*
   *  for every field on this endpoint, so there is nothing to send that would
   *  clear one; a task ranked wrongly is corrected by ranking it again. */
  setPriority(priority: Priority): void {
    this.change({ priority });
  }

  /** Close it without doing it. Undone by tapping `open`, like any other status
   *  — there is no separate "undrop", because there is no separate state to
   *  come back from. */
  drop(): void {
    this.change({ status: 'dropped' });
  }

  moveTo(assignee: Assignee): void {
    this.change({ assignee });
  }

  private change(change: Parameters<TasksApi['change']>[1]): void {
    const task = this.task();
    if (!task || this.saving()) return;
    this.saving.set(true);
    this.api.change(task.id, change).subscribe({
      next: () => {
        this.saving.set(false);
        // Re-read rather than patching the held object: the write also appends
        // to the history, and a page that showed the new status beside a
        // history that had not moved would be telling two stories. `load`
        // clears the failure on the way through.
        this.load();
        // The list this task came from is now out of date in the same way.
        this.store.refresh();
      },
      error: (err: unknown) => {
        this.saving.set(false);
        this.failed.set(reason(err));
      },
    });
  }
}
