import { Assignee, Priority, Status } from './models';

/**
 * How holders and statuses are drawn, one table each. `Record<Status, …>` makes
 * a new Rust member without a label, icon and colour a build error, the only
 * link there is: never loosen to `Partial<>` or a fallback.
 */
export const STATUS_LABEL: Record<Status, string> = {
  open: 'open',
  doing: 'in progress',
  done: 'done',
  dropped: 'dropped',
};

/**
 * What each rank means: a second copy of `Priority::gloss` in Rust, which
 * nothing checks, so change both or neither (P4's prompt clause is left out).
 * No entry for unranked, which is drawn as nothing.
 */
export const PRIORITY_GLOSS: Record<Priority, string> = {
  P0: 'damage is accruing — every hour it stays open costs more',
  P1: 'nothing is accruing, but other work is waiting on this',
  P2: 'ordinary work, nothing waiting on it — where UNRANKED sits',
  P3: 'a workaround exists and is in use; what it costs is friction',
  P4: 'nothing is being paid for it today',
};

/** Most urgent first, as offered. */
export const PRIORITIES: Priority[] = ['P0', 'P1', 'P2', 'P3', 'P4'];

export const STATUS_ICON: Record<Status, string> = {
  open: 'radio_button_unchecked',
  doing: 'pending',
  done: 'check_circle',
  // A cross, not a second tick: it reads "didn't happen" without reading "failed".
  dropped: 'cancel',
};

/** Blank counts as absent: `??` alone would draw a blank chip, read as unassigned. */
export function said(value: string | null | undefined): string | undefined {
  const trimmed = value?.trim();
  return trimmed === '' || trimmed === null ? undefined : trimmed;
}

/** What to call whoever is holding a task, in one word. */
export function holderLabel(assignee: Assignee): string {
  if (assignee.kind === 'nobody') return 'nobody';
  return said(assignee.name) ?? said(assignee.id) ?? 'nobody';
}

/** The same rule for a session row, whose fields are loose. */
export function sessionLabel(session: { id: string; name?: string }): string {
  return holderLabel({ kind: 'session', id: session.id, name: session.name });
}

/** The four buckets the list filters by, and what each means. */
export type Bucket = 'all' | 'mine' | 'sessions' | 'pile';

export const BUCKETS: Bucket[] = ['all', 'mine', 'sessions', 'pile'];

export const WHO_LABEL: Record<Bucket, string> = {
  all: 'everything',
  mine: 'mine',
  sessions: 'with a session',
  pile: 'in the pile',
};

/**
 * What the list is filtered to: a bucket, or one holder (so `/who` can open on
 * one). Prefixed `session:`/`person:` so an id can never collide with a bucket
 * word; the pile is the `pile` bucket only. Parsed at the edge, so an unknown
 * `?who=` is tested as "everything".
 */
export type Who =
  | { kind: 'bucket'; bucket: Bucket }
  | { kind: 'session'; id: string }
  | { kind: 'person'; id: string };

/** The default, and what an unreadable `?who=` falls back to. */
export const EVERYTHING: Who = { kind: 'bucket', bucket: 'all' };

/** `raw` as a bucket, or nothing. A `find` rather than an `includes` + cast. */
function asBucket(raw: string): Bucket | undefined {
  return BUCKETS.find((bucket) => bucket === raw);
}

/** Read `?who=`: every string is a selection, and an unknown one is everything. */
export function parseWho(raw: string | null | undefined): Who {
  if (!raw) return EVERYTHING;
  const bucket = asBucket(raw);
  if (bucket) return { kind: 'bucket', bucket };
  for (const kind of ['session', 'person'] as const) {
    const id = raw.startsWith(`${kind}:`) ? raw.slice(kind.length + 1) : '';
    // An empty id would show an empty list under a nameless chip: ignore it instead.
    if (id) return { kind, id };
  }
  return EVERYTHING;
}

/** The `?who=` value; `null` for the default, keeping it out of the URL. */
export function whoParam(who: Who): string | null {
  switch (who.kind) {
    case 'bucket':
      return who.bucket === 'all' ? null : who.bucket;
    case 'session':
      return `session:${who.id}`;
    case 'person':
      return `person:${who.id}`;
  }
}

/** The selection that shows one holder's work and nobody else's. */
export function focusOn(assignee: Assignee): Who {
  switch (assignee.kind) {
    case 'nobody':
      return { kind: 'bucket', bucket: 'pile' };
    case 'person':
      return assignee.id ? { kind: 'person', id: assignee.id } : EVERYTHING;
    case 'session':
      return assignee.id ? { kind: 'session', id: assignee.id } : EVERYTHING;
  }
}

/** Whether a task belongs in a selection. `me` is the signed-in person's id. */
export function inBucket(assignee: Assignee, who: Who, me: string | null): boolean {
  switch (who.kind) {
    case 'session':
      return assignee.kind === 'session' && assignee.id === who.id;
    case 'person':
      return assignee.kind === 'person' && assignee.id === who.id;
    case 'bucket':
      switch (who.bucket) {
        case 'all':
          return true;
        case 'mine':
          // The signed-in id: the allow-list is configuration.
          return assignee.kind === 'person' && (me === null || assignee.id === me);
        case 'sessions':
          return assignee.kind === 'session';
        case 'pile':
          return assignee.kind === 'nobody';
      }
  }
}
