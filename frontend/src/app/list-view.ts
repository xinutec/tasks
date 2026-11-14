import { Component, computed, effect, inject } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { MatButtonModule } from '@angular/material/button';
import { MatButtonToggleModule } from '@angular/material/button-toggle';
import { MatChipsModule } from '@angular/material/chips';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { map } from 'rxjs';

import {
  BUCKETS,
  Bucket,
  EVERYTHING,
  STATUS_ICON,
  STATUS_LABEL,
  WHO_LABEL,
  Who,
  holderLabel,
  inBucket,
  parseWho,
  whoParam,
} from './holder';
import { matches } from './search';
import { TaskStore } from './task-store';

const DAY = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'short' });
const DAY_UTC = new Intl.DateTimeFormat('en-GB', {
  day: 'numeric',
  month: 'short',
  timeZone: 'UTC',
});

/**
 * The open list in the backend's order (rank, then id), so lines don't move
 * while read; or the closed list, newest first. Whose, open or closed, and the
 * search all live in the URL, so a link or a back keeps them; all filter on
 * screen with no round trip.
 */
@Component({
  selector: 'app-list-view',
  templateUrl: './list-view.html',
  styleUrl: './list-view.scss',
  imports: [
    RouterLink,
    MatButtonModule,
    MatButtonToggleModule,
    MatChipsModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatProgressBarModule,
  ],
})
export class ListView {
  private store = inject(TaskStore);
  private route = inject(ActivatedRoute);
  private router = inject(Router);

  readonly statusIcon = STATUS_ICON;
  readonly statusLabel = STATUS_LABEL;
  readonly whoLabel = WHO_LABEL;
  readonly buckets = BUCKETS;

  private params = toSignal(this.route.queryParamMap);

  /** `?done=1`, the menu's "Finished work": the closed list instead of the open. */
  readonly closedView = computed(() => this.params()?.has('done') ?? false);
  readonly query = computed(() => this.params()?.get('q') ?? '');

  readonly loading = computed(() =>
    this.closedView() ? this.store.closed() === null && !this.failed() : this.store.loading(),
  );
  readonly failed = computed(() =>
    this.closedView() ? this.store.closedFailed() : this.store.failed(),
  );

  /** What the URL says to show. Anything unrecognised is `all`. */
  readonly who = toSignal(
    this.route.queryParamMap.pipe(map((params) => parseWho(params.get('who')))),
    { initialValue: EVERYTHING },
  );

  /**
   * The single holder shown, named from the tasks: arriving from `/who`, you were reading a name.
   */
  /** The bucket the toggle row shows as selected, or none while one holder is in focus. */
  readonly bucket = computed(() => {
    const who = this.who();
    return who.kind === 'bucket' ? who.bucket : null;
  });

  readonly focused = computed(() => {
    const who = this.who();
    if (who.kind === 'bucket') return null;
    const match = this.store.tasks().find((task) => inBucket(task.assignee, who, null));
    // A holder with nothing open still draws its chip, or "Nothing here" says nothing.
    return match ? holderLabel(match.assignee) : who.id;
  });

  readonly shown = computed(() => {
    const source = this.closedView() ? (this.store.closed() ?? []) : this.store.tasks();
    return source.filter(
      (task) =>
        // While `/api/me` loads, `inBucket` treats null as any person, so the list isn't
        // briefly empty.
        inBucket(task.assignee, this.who(), this.store.personId()) && matches(task, this.query()),
    );
  });

  /**
   * A row draws a holder or a filer, never both: the filer fills the space a pile
   * task leaves, exactly where a session scanning the pile has to decide.
   */
  readonly rows = computed(() =>
    this.shown().map((task) => ({
      task,
      holder: holderLabel(task.assignee),
      filer: task.assignee.kind === 'nobody' ? (task.filed_by ?? null) : null,
      // A deadline is a day, so it is read in UTC or it shifts by one west of it.
      due: task.due ? DAY_UTC.format(new Date(task.due)) : null,
      closed: task.closed_at ? DAY.format(new Date(task.closed_at)) : null,
    })),
  );

  readonly doing = computed(() => this.shown().filter((t) => t.status === 'doing').length);

  constructor() {
    this.store.ensure();
    effect(() => {
      if (this.closedView()) this.store.ensureClosed();
    });
  }

  pickClosed(closed: boolean): void {
    this.setParams({ done: closed ? '1' : null });
  }

  search(text: string): void {
    this.setParams({ q: text === '' ? null : text });
  }

  /**
   * Navigate, so the selection survives a reload and can be linked; `replaceUrl`
   * keeps back meaning the previous screen, not the previous keystroke. Merged,
   * so picking one filter keeps the others.
   */
  private setParams(queryParams: Record<string, string | null>): void {
    void this.router.navigate([], {
      relativeTo: this.route,
      queryParams,
      queryParamsHandling: 'merge',
      replaceUrl: true,
    });
  }

  pickWho(who: Who): void {
    this.setParams({ who: whoParam(who) });
  }

  /** The chips set a bucket; the focused chip clears back to everything. */
  pickBucket(bucket: Bucket): void {
    this.pickWho({ kind: 'bucket', bucket });
  }

  clearFocus(): void {
    this.pickWho(EVERYTHING);
  }
}
