import { Component, computed, inject, signal } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { RouterLink } from '@angular/router';

import { STATUS_ICON, STATUS_LABEL, WHO_LABEL, Who, holderLabel, inBucket } from './holder';
import { RepoCount, Task } from './models';
import { TasksApi } from './tasks-api';

/**
 * The open list.
 *
 * **Grouped by repository, ordered by id inside each group.** The backend
 * returns creation order deliberately — a list that re-sorts as work starts on
 * an item moves the line you were reading — so the grouping happens here, where
 * it is presentation and can change without the API changing.
 *
 * The two filters answer the two questions actually asked of this screen:
 * *which project* and *whose is it*. Both are held in the URL, so a filtered
 * list is a link.
 */
@Component({
  selector: 'app-list-view',
  templateUrl: './list-view.html',
  styleUrl: './list-view.scss',
  imports: [RouterLink, MatButtonModule, MatIconModule, MatProgressBarModule],
})
export class ListView {
  private api = inject(TasksApi);

  readonly statusIcon = STATUS_ICON;
  readonly statusLabel = STATUS_LABEL;
  readonly whoLabel = WHO_LABEL;
  readonly buckets: Who[] = ['all', 'mine', 'sessions', 'pile'];

  readonly loading = signal(true);
  readonly failed = signal<string | null>(null);
  readonly tasks = signal<Task[]>([]);
  readonly repos = signal<RepoCount[]>([]);

  readonly repo = signal<string | null>(null);
  readonly who = signal<Who>('all');

  /** Who the signed-in person is, so `mine` means them and not a hard-coded
   *  username. Null while `/api/me` is in flight, which `inBucket` treats as
   *  "any person" rather than "nobody" — a list that is briefly empty reads as
   *  no work. */
  readonly me = signal<string | null>(null);

  readonly shown = computed(() =>
    this.tasks().filter((task) => {
      if (this.repo() !== null && (task.repo ?? null) !== this.repo()) return false;
      return inBucket(task.assignee, this.who(), this.me());
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
      tasks,
    }));
  });

  readonly doing = computed(() => this.shown().filter((t) => t.status === 'doing').length);

  constructor() {
    this.api.me().subscribe({
      next: (me) => this.me.set(me.kind === 'person' ? me.id : null),
      error: () => this.me.set(null),
    });
    this.api.list().subscribe({
      next: (tasks) => {
        this.tasks.set(tasks);
        this.loading.set(false);
      },
      error: () => {
        this.failed.set('The tasks service did not answer.');
        this.loading.set(false);
      },
    });
    this.api.repos().subscribe({
      next: (repos) => this.repos.set(repos),
      error: () => this.repos.set([]),
    });
  }

  holder(task: Task): string {
    return holderLabel(task.assignee);
  }

  pickRepo(repo: string | null): void {
    this.repo.set(this.repo() === repo ? null : repo);
  }

  pickWho(who: Who): void {
    this.who.set(who);
  }
}
