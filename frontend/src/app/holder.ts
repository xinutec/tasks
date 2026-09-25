import { Assignee, Priority, Status } from './models';

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
  dropped: 'dropped',
};

/**
 * What each rank means, in one line: a TEST that can fail, applied as a cascade
 * — see `Priority::gloss` in Rust.
 *
 * ⚠ **A second copy of `Priority::gloss`, and it has to say the same thing**,
 * or Pippijn and the sessions read the levels differently. Nothing checks this
 * at build time, so change both or neither. (P4's session-only clause about
 * the prompt is left out here.)
 *
 * ⚠ **No entry for "unranked", deliberately**: absence sorts where `P2` does,
 * and is drawn as nothing at all.
 */
export const PRIORITY_GLOSS: Record<Priority, string> = {
  P0: 'damage is accruing — every hour it stays open costs more',
  P1: 'nothing is accruing, but other work is waiting on this',
  P2: 'ordinary work, nothing waiting on it — where UNRANKED sits',
  P3: 'a workaround exists and is in use; what it costs is friction',
  P4: 'nothing is being paid for it today',
};

/** Most urgent first, which is the order they are offered in. */
export const PRIORITIES: Priority[] = ['P0', 'P1', 'P2', 'P3', 'P4'];

export const STATUS_ICON: Record<Status, string> = {
  open: 'radio_button_unchecked',
  doing: 'pending',
  done: 'check_circle',
  // Not a second tick in another colour: a glance has to say which closing it
  // was, and a cross reads as "did not happen" without reading as "failed".
  dropped: 'cancel',
};

/**
 * A field that is present but blank is absent: `??` alone would draw a blank
 * chip, which reads as unassigned.
 */
export function said(value: string | null | undefined): string | undefined {
  const trimmed = value?.trim();
  return trimmed === '' || trimmed === null ? undefined : trimmed;
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
 * What the list is filtered to: one of the buckets, or ONE named holder — so
 * `/who`'s `hardware 6/31` can open onto those six.
 *
 * **Prefixed, rather than bare ids.** `session:<id>` and `person:<id>` cannot
 * collide with a bucket word, and a bare id could: `pippijn` is a person today,
 * and nothing stops a session from being named or identified as `mine` or
 * `all`. The prefix also makes the URL say what it means when read aloud.
 *
 * There is no `nobody:` — the pile is a bucket already, and two spellings for
 * one selection is how they drift apart.
 *
 * ⚠ **Parsed, not asserted.** Casting the query parameter into a union would
 * type `?who=garbage` as a valid selection that filters nothing; one parse at
 * the edge makes "anything unrecognised is everything" a tested behaviour.
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

/**
 * Read `?who=`. Total by construction: every string is a selection, and one
 * nobody meant is everything rather than nothing.
 */
export function parseWho(raw: string | null | undefined): Who {
  if (!raw) return EVERYTHING;
  const bucket = asBucket(raw);
  if (bucket) return { kind: 'bucket', bucket };
  for (const kind of ['session', 'person'] as const) {
    const id = raw.startsWith(`${kind}:`) ? raw.slice(kind.length + 1) : '';
    // An empty id would match no holder, so the screen would show an empty list
    // under a chip with no name in it — worse than ignoring the parameter.
    if (id) return { kind, id };
  }
  return EVERYTHING;
}

/** The `?who=` value for a selection. `null` for the default, to keep it out
 *  of the URL entirely. */
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
          // The signed-in id, not a hard-coded `pippijn`: the allow-list is
          // configuration.
          return assignee.kind === 'person' && (me === null || assignee.id === me);
        case 'sessions':
          return assignee.kind === 'session';
        case 'pile':
          return assignee.kind === 'nobody';
      }
  }
}
