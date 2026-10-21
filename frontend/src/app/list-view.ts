import { Component, computed, inject } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { MatButtonModule } from '@angular/material/button';
import { MatButtonToggleModule } from '@angular/material/button-toggle';
import { MatChipsModule } from '@angular/material/chips';
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
 * The open list, flat, in the backend's order (rank, then id), so lines don't
 * move while read. The *whose* filter runs on screen, with no round trip, and
 * lives in the URL so `/who` can link to one holder.
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
    MatIconModule,
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

  readonly loading = this.store.loading;
  readonly failed = this.store.failed;

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

  readonly shown = computed(() =>
    this.store.tasks().filter((task) =>
      // While `/api/me` loads, `inBucket` treats null as any person, so the list isn't
      // briefly empty.
      inBucket(task.assignee, this.who(), this.store.personId()),
    ),
  );

  /**
   * A row draws a holder or a filer, never both: the filer fills the space a pile
   * task leaves, exactly where a session scanning the pile has to decide.
   */
  readonly rows = computed(() =>
    this.shown().map((task) => ({
      task,
      holder: holderLabel(task.assignee),
      filer: task.assignee.kind === 'nobody' ? (task.filed_by ?? null) : null,
    })),
  );

  readonly doing = computed(() => this.shown().filter((t) => t.status === 'doing').length);

  constructor() {
    this.store.ensure();
  }

  /**
   * Navigate, so the selection survives a reload and can be linked; `replaceUrl`
   * keeps back meaning the previous screen, not the previous chip.
   */
  pickWho(who: Who): void {
    void this.router.navigate([], {
      relativeTo: this.route,
      queryParams: { who: whoParam(who) },
      replaceUrl: true,
    });
  }

  /** The chips set a bucket; the focused chip clears back to everything. */
  pickBucket(bucket: Bucket): void {
    this.pickWho({ kind: 'bucket', bucket });
  }

  clearFocus(): void {
    this.pickWho(EVERYTHING);
  }
}
