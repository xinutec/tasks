import { Component, computed, inject, signal } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { RouterLink } from '@angular/router';

import { STATUS_ICON, STATUS_LABEL, WHO_LABEL, Who, holderLabel, inBucket } from './holder';
import { Task } from './models';
import { TaskStore } from './task-store';

/**
 * The open list.
 *
 * **Grouped by repository, ordered by id inside each group.** The backend
 * returns creation order deliberately — a list that re-sorts as work starts on
 * an item moves the line you were reading — so the grouping happens here, where
 * it is presentation and can change without the API changing.
 *
 * The two filters answer the two questions actually asked of this screen:
 * *which project* and *whose is it*. Both are held as state on the screen rather
 * than as a query the backend runs, because the whole list is already here and a
 * round trip to hide four rows is a round trip a phone waits for.
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
  readonly repos = this.store.repos;

  readonly repo = signal<string | null>(null);
  readonly who = signal<Who>('all');

  readonly shown = computed(() =>
    this.store.tasks().filter((task) => {
      if (this.repo() !== null && (task.repo ?? null) !== this.repo()) return false;
      // `personId()` is null while `/api/me` is in flight, which `inBucket`
      // treats as "any person" rather than "nobody" — a list that is briefly
      // empty reads as no work.
      return inBucket(task.assignee, this.who(), this.store.personId());
    }),
  );

  /** The shown tasks grouped by repository, in the order the repositories first
   *  appear — which is id order, so the grouping does not reorder anything. */
  readonly groups = computed(() => {
    const groups = new Map<string, Task[]>();
    for (const task of this.shown()) {
      const key = task.repo ?? '';
      const held = groups.get(key);
      if (held) held.push(task);
      else groups.set(key, [task]);
    }
    return [...groups.entries()].map(([repo, tasks]) => ({
      // The empty key is the pile of tasks belonging to no checkout — named
      // rather than left blank, because a heading of nothing reads as a bug.
      repo: repo || 'no repo',
      tasks: tasks.map((task) => ({ task, holder: holderLabel(task.assignee) })),
    }));
  });

  readonly doing = computed(() => this.shown().filter((t) => t.status === 'doing').length);

  constructor() {
    this.store.ensure();
  }

  pickRepo(repo: string | null): void {
    this.repo.set(this.repo() === repo ? null : repo);
  }

  pickWho(who: Who): void {
    this.who.set(who);
  }
}
