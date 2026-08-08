import { Assignee, Status } from './models';

/**
 * How a holder and a status are drawn — one table each, keyed by the closed
 * vocabulary.
 *
 * ⚠ **`Record<Status, …>` is the point.** The unions are restated from Rust by
 * hand (see `models.ts`), so nothing links the two at compile time — but a new
 * member added here without a label, an icon and a colour is a *build* error,
 * which is the nearest thing to a link there is. Do not loosen these to
 * `Partial<>` or index them with a fallback.
 */
export const STATUS_LABEL: Record<Status, string> = {
  open: 'open',
  doing: 'in progress',
  done: 'done',
};

export const STATUS_ICON: Record<Status, string> = {
  open: 'radio_button_unchecked',
  doing: 'pending',
  done: 'check_circle',
};

/**
 * A field that is present but blank is absent.
 *
 * The distinction matters one line below: `??` alone would return an empty
 * name, leaving a blank chip — which reads as unassigned, and is a lie about a
 * task somebody is holding.
 */
function said(value: string | undefined): string | undefined {
  const trimmed = value?.trim();
  return trimmed === '' ? undefined : trimmed;
}

/** What to call whoever is holding a task, in one word. */
export function holderLabel(assignee: Assignee): string {
  if (assignee.kind === 'nobody') return 'nobody';
  return said(assignee.name) ?? said(assignee.id) ?? 'nobody';
}

/** The same rule for a session row, which has the fields loose rather than in
 *  an `Assignee`. */
export function sessionLabel(session: { id: string; name?: string }): string {
  return holderLabel({ kind: 'session', id: session.id, name: session.name });
}

/** The three buckets the list filters by, and what each means. */
export type Who = 'all' | 'mine' | 'sessions' | 'pile';

export const WHO_LABEL: Record<Who, string> = {
  all: 'everything',
  mine: 'mine',
  sessions: 'with a session',
  pile: 'in the pile',
};

/** Whether a task belongs in a bucket. `me` is the signed-in person's id. */
export function inBucket(assignee: Assignee, who: Who, me: string | null): boolean {
  switch (who) {
    case 'all':
      return true;
    case 'mine':
      // Compared against the signed-in id rather than hard-coding `pippijn`:
      // the allow-list is configuration, and a view that assumes a username is
      // one that breaks silently the day it changes.
      return assignee.kind === 'person' && (me === null || assignee.id === me);
    case 'sessions':
      return assignee.kind === 'session';
    case 'pile':
      return assignee.kind === 'nobody';
  }
}
