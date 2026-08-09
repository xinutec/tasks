import { Component, computed, inject } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
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
 *
 * ⚠ **The filter lives in the URL, and that is what makes `/who` able to link
 * here.** It was a component signal until #657, which meant the only way to
 * reach a selection was to tap a chip: `/who` could say `hardware 6/31` and had
 * no way to show you which six, because there was no address for "hardware's
 * work". `app.routes.ts` had documented `?who=` for weeks — a reader checking
 * whether the feature existed found a sentence saying it did.
 */
@Component({
  selector: 'app-list-view',
  templateUrl: './list-view.html',
  styleUrl: './list-view.scss',
  imports: [RouterLink, MatButtonModule, MatIconModule, MatProgressBarModule],
})
export class ListView {
  private store = inject(TaskStore);
  private route = inject(ActivatedRoute);
  private router = inject(Router);

  readonly statusIcon = STATUS_ICON;
  readonly statusLabel = STATUS_LABEL;
  readonly whoLabel = WHO_LABEL;
  readonly buckets = BUCKETS;

  readonly loading = this.store.loading;
  readonly failed = this.store.failed;

  /** What the URL says to show. Anything unrecognised is `all`. */
  readonly who = toSignal(
    this.route.queryParamMap.pipe(map((params) => parseWho(params.get('who')))),
    { initialValue: EVERYTHING },
  );

  /**
   * The holder being shown, when it is one holder rather than a bucket.
   *
   * Named from the tasks themselves rather than from the id in the URL: a
   * session's id is 36 characters of uuid, and the whole point of arriving here
   * from `/who` is that you were reading a name.
   */
  readonly focused = computed(() => {
    const who = this.who();
    if (who.kind === 'bucket') return null;
    const match = this.store.tasks().find((task) => inBucket(task.assignee, who, null));
    // A holder with nothing open still has to draw its own chip, or the screen
    // says "Nothing here" with no indication of what it was asked for.
    return match ? holderLabel(match.assignee) : who.id;
  });

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

  /**
   * Navigate rather than assign, so the selection survives a reload and can be
   * linked to. `replaceUrl` keeps the back button meaning "the screen before
   * this one" instead of walking back through every chip that was tapped.
   */
  pickWho(who: Who): void {
    void this.router.navigate([], {
      relativeTo: this.route,
      queryParams: { who: whoParam(who) },
      replaceUrl: true,
    });
  }

  isOn(bucket: Bucket): boolean {
    const who = this.who();
    return who.kind === 'bucket' && who.bucket === bucket;
  }

  /** The chips set a bucket; the focused chip clears back to everything. */
  pickBucket(bucket: Bucket): void {
    this.pickWho({ kind: 'bucket', bucket });
  }

  clearFocus(): void {
    this.pickWho(EVERYTHING);
  }
}
