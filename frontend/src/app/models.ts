/**
 * What the backend sends.
 *
 * ⚠ **Restated by hand, not generated.** The vocabularies here — `Status` and
 * `AssigneeKind` — are closed sets the Rust side also enumerates, and nothing
 * links the two at compile time. `Record<Status, …>` in `holder.ts` is what
 * turns a missed member into a build error on this side; the backend's own
 * exhaustive `match` does the same over there. dev-lint's `DL-WIRE-MIRROR-DRIFT`
 * covers the rest: it reads both files and fails when a field's optionality or
 * nullability disagrees, which is why the shapes below are spelled out so
 * fussily. `?` mirrors `skip_serializing_if`; `| null` mirrors `Option`.
 */

/** Where a task stands. Mirrors `tasks::types::Status`.
 *
 *  Two open states and two ways out: `dropped` is closed without being done —
 *  overtaken, obsolete, decided against — so no list credits anybody with work
 *  nobody did. */
export type Status = 'open' | 'doing' | 'done' | 'dropped';

/** Who holds a task. Mirrors `tasks::types::AssigneeKind`. */
export type AssigneeKind = 'nobody' | 'person' | 'session';

/** How urgent, when somebody has said. Mirrors `tasks::types::Priority`.
 *
 *  ⚠ **Absence is NOT a sixth member and must not be given one**: most tasks
 *  are unranked, and a default would have them all assert something nobody
 *  said. An unranked task sorts where `P2` does, so `P3` and `P4` sit *below*
 *  the untriaged. The backend sorts; nothing here reorders. */
export type Priority = 'P0' | 'P1' | 'P2' | 'P3' | 'P4';

export interface Assignee {
  kind: AssigneeKind;
  /** The Nextcloud user id, or the CLI session id. Absent for `nobody`. */
  id?: string;
  /** What to call them. A session may not have named itself yet. */
  name?: string;
}

/** A task in a list. Deliberately has no `body` — see `TaskDetail`. */
export interface Task {
  id: number;
  subject: string;
  status: Status;
  /** Absent on almost everything — see `Priority`. */
  priority?: Priority;
  /**
   * The day it has to be done by, `YYYY-MM-DD`. Absent on almost everything.
   *
   * ⚠ **Do not sort by it here.** A deadline is evidence for a rank; inside the
   * last week the backend raises it — see `escalated_to`.
   */
  due?: string;
  /**
   * What this sorts as instead, when a near deadline has raised it — always
   * `'P0'` when present, absent otherwise.
   *
   * ⚠ **Draw `escalated_to ?? priority`, and do not work the rule out here**:
   * the window and the level live in SQL, and a client recomputing it would be
   * a second opinion about what day it is.
   *
   * ⚠ **`priority` still holds what somebody actually chose** — nothing is
   * written when a deadline comes close. This is derived on every read.
   */
  escalated_to?: Priority;
  /** Whether `due` has passed, by the SERVER's clock. Absent means false — and
   *  do not recompute it from `due` here, or two clients in two timezones will
   *  disagree about which day it is. */
  overdue?: boolean;
  /**
   * The tasks this one waits for, oldest id first. Absent when there are none.
   *
   * ⚠ **Not the same as `blocked`.** The link is kept when a blocker closes,
   * because the dependency is a fact about how the work went; what ends is its
   * effect. `blocked` is the question a reader is actually asking, and the
   * backend answers it — do not recompute it here, since it depends on the
   * status of rows this client may not have.
   */
  blocked_on?: number[];
  /** Whether any blocker is still open. Absent means false. */
  blocked?: boolean;
  assignee: Assignee;
  /** Whether there is prose behind it worth opening. */
  detailed: boolean;
  /**
   * How many lines the body prints as, `0` when there is none.
   *
   * For the CLI, where a read piped to `head` otherwise looks complete. Carried
   * here because the wire carries it; no view needs to draw it.
   */
  body_lines: number;
  /**
   * What the session that filed it calls itself. Absent when Pippijn filed it,
   * or when the filing session had never named itself — both mean "not said",
   * which is why it is drawn only where there is no holder.
   */
  filed_by?: string;
  /**
   * How long this body was when a model last read it and had something to say.
   * Absent when nothing is outstanding, which is nearly every task.
   *
   * ⚠ **The number, not the words.** The critique is on the detail
   * (`sprawl_said`) because a list must not carry prose — the same trade
   * `detailed` makes. Draw it as a mark, not as text.
   */
  sprawl_chars?: number;
  created_at: string;
  updated_at: string;
  closed_at?: string;
}

/**
 * A task after a write, and what the write actually moved: `changed` holds the
 * `task_events` kinds written, and is empty when the call moved nothing.
 */
export interface Updated extends Task {
  changed: string[];
  /**
   * What this write overwrote, present only when it moved prose.
   *
   * Absent — not null — for a change that only moved a status, a rank or a
   * holder.
   */
  replaced?: Replaced;
}

/**
 * The provenance of text an edit landed on — shown at the moment of the write,
 * refusing nothing. See `tasks::types::Replaced`. The remedy is
 * `GET /api/tasks/{id}/previous` and a write-back.
 */
export interface Replaced {
  at: string;
  by: string;
  /** Body length before and after, in characters. */
  was: number;
  now: number;
  /** Characters grown since the last edit that made the body smaller — this one
   *  included. Zero on the edit that consolidates. */
  accreted: number;
}

export interface TaskEvent {
  at: string;
  kind: string;
  detail?: string | null;
  actor: string;
}

export interface TaskDetail extends Task {
  body: string;
  body_html: string;
  events: TaskEvent[];
  /** Whether an edit has replaced text here, so there is a version to put back —
   *  so the page need not fetch a whole previous body to offer the button. */
  restorable: boolean;
  /**
   * What a model last said about this body, verbatim, when it had something to
   * say. Absent when nothing is outstanding.
   *
   * Kept so whoever opens the task reads it, not only the session whose edit
   * prompted it.
   */
  sprawl_said?: string | null;
}

/** A task as it stood before its most recent edit — `GET /api/tasks/{id}/previous`. */
export interface Revision {
  at: string;
  actor: string;
  /** Whether the edit this would revert was made by whoever is asking.
   *
   *  ⚠ **Restoring reverts THE last edit, not YOUR last edit** — one version is
   *  kept per task, not per actor. The CLI refuses on this and takes `--anyway`,
   *  because it has nothing on screen to read. This page does not: it shows
   *  `Replaced <when> by <actor>` above the two buttons, so the same fact is in
   *  front of whoever taps, before they tap. */
  mine: boolean;
  subject: string;
  body: string;
}

export interface Session {
  id: string;
  name?: string;
  first_seen: string;
  last_seen: string;
  open: number;
}

/**
 * One party's share of the work, as `/api/holders` answers.
 *
 * ⚠ **`total` counts finished work, which is why this is not `Session`**: with
 * `open` alone, a cleared plate reads as an idle one. Both come from the
 * backend, counted once.
 */
export interface Holder {
  /** The same closed vocabulary as an assignee's. */
  kind: 'session' | 'person' | 'nobody';
  id?: string;
  name?: string;
  open: number;
  total: number;
}

/** Who the caller is, as `/api/me` answers. */
export interface Me {
  kind: 'person' | 'session';
  id: string;
  name?: string;
}

/**
 * A task being filed.
 *
 * A REQUEST body: this side serialises it and Rust deserialises it, so the
 * mirror runs backwards. ⚠ Omitting `assignee` files the task to the FILER, not
 * the pile — see `NewTask` in `tasks/repo.rs`.
 */
export interface NewTask {
  subject: string;
  body: string;
  /**
   * Whether the filer let the duplicate check run.
   *
   * ⚠ **Required here, and said rather than left out.** The service refuses
   * `false` unless that session was just refused this exact subject. The web UI
   * cannot skip the check, so it says `true`.
   */
  checked: boolean;
  /**
   * ⚠ **Required, and the only field here that is** — note there is no `?`.
   * Every other key means *leave it alone* when absent; this one has no such
   * reading, and the service refuses a filing that never mentions it.
   *
   * * a level — judged.
   * * `null` — **unassessed**: nobody has judged this yet.
   *
   * Both sort at `P2`; what the pair buys is that `P2` means *somebody looked
   * and called it ordinary*. Send `null` rather than omitting the key —
   * omitting it is a **400** naming both answers, not a default (`src/wire.rs`).
   */
  // dev-lint: allow-wire-mirror the Rust side is `Ranking`, not `Option<Priority>`, and the rule reads the null arm off the TYPE. `Ranking` is a hand-written Deserialize whose whole purpose is that `null` is legal and ABSENCE is not — the one shape an Option cannot express. Null is right here; the rule cannot see it.
  priority: Priority | null;
  due?: string | null;
  blocked_on?: number[];
  assignee?: Assignee;
  /**
   * Why this is nobody's — **required when `assignee` is the pile, refused
   * otherwise**, so the two travel together or not at all.
   *
   * ⚠ **Filings to the pile were almost always corrected later**, so it is
   * argued for rather than typed. Omitted with a `nobody` assignee, or sent
   * with a real holder, it is a **400**. Whitespace is not an answer.
   */
  spare?: string;
}

/**
 * A partial change. An absent field means *leave it alone* — which is the whole
 * semantics: a client changing a status must not have to restate a body it has
 * never read.
 */
export interface Change {
  subject?: string;
  body?: string;
  /**
   * Text to put above the existing body, keeping all of it.
   *
   * ⚠ **Resolved server-side, against the body inside the same transaction that
   * reads it.** Do NOT reimplement it here as read-concatenate-PATCH: that is a
   * read-modify-write across two round trips, and two clients adding to one
   * task would drop one of the two additions.
   *
   * Sending this together with `body` is a **400** — one replaces the body and
   * the other keeps it.
   */
  prepend?: string;
  /** Text to put below the existing body. The twin of `prepend`; both may be
   *  sent at once, and the same rules apply. */
  append?: string;
  status?: Status;
  /** This cannot UNRANK a task; ranking it again is the correction. */
  priority?: Priority;
  /** The blockers as they should now be — the whole set. `[]` unblocks. */
  blocked_on?: number[];
  /** Set the day it has to be done by. */
  due?: string;
  /** Take it off. A date has no "empty" value the way a blocker list does, so
   *  removing one needs its own field rather than a meaningful null. */
  clear_due?: boolean;
  assignee?: Assignee;
  /** Say that a `body` keeping almost nothing of the one it replaces is meant.
   *  Without it the server refuses that write, because it is far more often a
   *  mistake than an edit.
   *
   *  ⚠ **Restoring sets this, and has to.** Putting back what an edit replaced
   *  is a change of subject and body like any other, so undoing an edit that
   *  *grew* a body sends a shorter one — the exact shape the guard refuses. */
  replace_body?: boolean;
}
