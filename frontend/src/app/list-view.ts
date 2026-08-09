import { Component, computed, inject, signal } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { RouterLink } from '@angular/router';

import { STATUS_ICON, STATUS_LABEL, WHO_LABEL, Who, holderLabel, inBucket } from './holder';
import { TaskStore } from './task-store';

/**
 * The open list.
 *
 * **One flat list, in id order.** The backend returns creation order
 * deliberately — a list that re-sorts as work starts on an item moves the line
 * you were reading — and this screen keeps it.
 *
 * It grouped by repository until the column was dropped: a session spans
 * checkouts, so the repository was never a question with one answer. What is
 * left is *whose is it*, held as state on the screen rather than as a query the
 * backend runs, because the whole list is already here and a round trip to hide
 * four rows is a round trip a phone waits for.
 */
@Component({
  selector: 'app-list-view',
  templateUrl: './list-view.html',
  styleUrl: './list-view.scss',
  imports: [RouterLink, MatButtonModule, MatIconModule, MatProgressBarModule],
})
export class ListView {
  private store = inject(TaskStore);

  readonly statusIcon = STATUS_ICON;
  readonly statusLabel = STATUS_LABEL;
  readonly whoLabel = WHO_LABEL;
  readonly buckets: Who[] = ['all', 'mine', 'sessions', 'pile'];

  readonly loading = this.store.loading;
  readonly failed = this.store.failed;

  readonly who = signal<Who>('all');

  readonly shown = computed(() =>
    this.store.tasks().filter((task) =>
      // `personId()` is null while `/api/me` is in flight, which `inBucket`
      // treats as "any person" rather than "nobody" — a list that is briefly
      // empty reads as no work.
      inBucket(task.assignee, this.who(), this.store.personId()),
    ),
  );

  readonly rows = computed(() =>
    this.shown().map((task) => ({ task, holder: holderLabel(task.assignee) })),
  );

  readonly doing = computed(() => this.shown().filter((t) => t.status === 'doing').length);

  constructor() {
    this.store.ensure();
  }

  pickWho(who: Who): void {
    this.who.set(who);
  }
}
