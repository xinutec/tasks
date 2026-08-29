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
 *  Two open states and two ways out: `dropped` is a task closed without being
 *  done — overtaken, obsolete, decided against — and it is a separate word from
 *  `done` so that no list credits anybody with work nobody did. */
export type Status = 'open' | 'doing' | 'done' | 'dropped';

/** Who holds a task. Mirrors `tasks::types::AssigneeKind`. */
export type AssigneeKind = 'nobody' | 'person' | 'session';

/** How urgent, when somebody has said. Mirrors `tasks::types::Priority`.
 *
 *  ⚠ **Absence is NOT a sixth member and must not be given one.** Almost every
 *  task is unranked and always will be — there were 700-odd rows the day this
 *  was added and none of them were going to be triaged — so a default would
 *  have all of them assert something nobody said. An unranked task sorts where
 *  `P2` does, which is what lets `P3` and `P4` mean *below the untriaged*
 *  rather than *above it*. The backend does that sorting; nothing here
 *  reorders. */
export type Priority = 'P0' | 'P1' | 'P2' | 'P3' | 'P4';

export interface Assignee {
  kind: AssigneeKind;
  /** The Nextcloud user id, or the CLI session id. Absent for `nobody`. */
  id?: string | null;
  /** What to call them. A session may not have named itself yet. */
  name?: string | null;
}

/** A task in a list. Deliberately has no `body` — see `TaskDetail`. */
export interface Task {
  id: number;
  subject: string;
  status: Status;
  /** Absent on almost everything — see `Priority`. */
  priority?: Priority | null;
  /**
   * The day it has to be done by, `YYYY-MM-DD`. Absent on almost everything.
   *
   * ⚠ **It does not reorder anything** — the backend sorts by priority and this
   * is not part of that. A deadline is evidence for a rank, not a competing
   * answer to *what next*, so do not sort by it here either.
   */
  due?: string | null;
  /**
   * What this sorts as instead, when a near deadline has raised it — always
   * `'P0'` when present, absent otherwise.
   *
   * ⚠ **Draw `escalated_to ?? priority`, and do not work the rule out here.**
   * The week (less than seven days) and the level both live in SQL, so this
   * carries the VALUE rather than a flag. A client that recomputed it would be
   * a second copy of the rule and a second opinion about what day it is.
   *
   * ⚠ **`priority` still holds what somebody actually chose** — nothing is
   * written when a deadline comes close. This is derived on every read.
   */
  escalated_to?: Priority | null;
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
   * What the session that filed it calls itself. Absent when Pippijn filed it,
   * or when the filing session had never named itself — both mean "not said",
   * which is why it is drawn only where there is no holder.
   */
  filed_by?: string | null;
  /**
   * How long this body was when a model last read it and had something to say.
   * Absent when nothing is outstanding, which is nearly every task.
   *
   * ⚠ **The number, not the words.** The critique is on the detail
   * (`sprawl_said`) because a list must not carry prose — the same trade
   * `detailed` makes. Draw it as a mark, not as text.
   */
  sprawl_chars?: number | null;
  created_at: string;
  updated_at: string;
  closed_at?: string | null;
}

/**
 * A task after a write, and what the write actually moved.
 *
 * `changed` holds the `task_events` kinds written — `status`, `assigned`,
 * `edited` — and is empty when the call moved nothing. A no-op is often the
 * right answer, so it is reported rather than refused; what it must not do is
 * answer exactly like a write that worked.
 */
export interface Updated extends Task {
  changed: string[];
  /**
   * What this write overwrote, present only when it moved prose.
   *
   * Absent — not null — for a change that only moved a status, a rank or a
   * holder, which is the shape the server sends and the reason this is
   * optional rather than nullable.
   */
  replaced?: Replaced;
}

/**
 * The provenance of text an edit landed on: when it was last written, by whom,
 * and how much of it there was before and after.
 *
 * ⚠ **It exists to be shown at the moment of the write, and it refuses
 * nothing.** A body rewritten from a stale copy looks exactly like a correct
 * edit until somebody sees that the text being replaced was written by another
 * holder more recently than the writer believed. `task undo <id>` is the
 * remedy; the API route behind it is `GET /api/tasks/{id}/previous`.
 */
export interface Replaced {
  at: string;
  by: string;
  /** Body length before and after, in characters. */
  was: number;
  now: number;
  /**
   * How much this body has grown, in characters, since the last edit that made
   * it smaller — this one included.
   *
   * ⚠ **Neither a size nor a count of edits.** An absolute size cannot tell a
   * long body somebody has just rewritten from a short one that has doubled
   * since anyone read it, and a count of edits cannot tell three typo fixes
   * from three two-thousand-character dumps. What this measures is the text
   * nobody has read as a whole, which is the text that goes stale in place.
   *
   * Zero on the edit that consolidates, because that edit is the answer.
   */
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
  /** Whether an edit has replaced text here, so there is a version to put back.
   *  Answered by the server so the page need not fetch a whole previous body to
   *  decide whether to offer the button. */
  restorable: boolean;
  /**
   * What a model last said about this body, verbatim, when it had something to
   * say. Absent when nothing is outstanding.
   *
   * ⚠ **Kept because it used to evaporate.** It was printed once, as the tail
   * of a successful edit, to whoever happened to be making that edit — and then
   * it was gone. Here it is read by whoever opens the task, which is who it was
   * always addressed to.
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
  name?: string | null;
  first_seen: string;
  last_seen: string;
  open: number;
}

/**
 * One party's share of the work, as `/api/holders` answers.
 *
 * ⚠ **`total` counts finished work, and that is why this is not `Session`.**
 * `open` alone says who is busy and nothing about who has done anything, because
 * a task leaves `open` the moment it is finished — so a session that has cleared
 * its plate reads as an idle one. Both numbers come from the backend rather than
 * being derived here: two figures that must agree should be counted once.
 */
export interface Holder {
  /** The same closed vocabulary as an assignee's. */
  kind: 'session' | 'person' | 'nobody';
  id?: string | null;
  name?: string | null;
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
 * A REQUEST body: this side serialises it and Rust deserialises it, which is the
 * one place the mirror runs backwards. Omitting `assignee` is what "leave it in
 * the pile" means, so it is optional — see the note on `NewTask` in
 * `tasks/repo.rs` for how that is stated on the other side.
 */
export interface NewTask {
  subject: string;
  body: string;
  /**
   * Whether the filer let the duplicate check run.
   *
   * ⚠ **Required here, and said rather than left out.** The service refuses a
   * filing that says `false` unless that session has just been refused this
   * exact subject — `--no-duplicate-check` may overrule a refusal, never
   * pre-empt one. Rust defaults an absent key to `true` so that an older client
   * is not silently exempted, but this side has no reason to lean on that: the
   * web UI cannot skip the check, so it says `true` and means it.
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
   * Both sort at `P2`. What the pair buys is that `P2` means *somebody looked
   * and called it ordinary*, where it used to be indistinguishable from
   * *nobody looked*. Send `null` rather than omitting the key — omitting it is
   * a **400** naming both answers, not a default:
   *
   * > `priority` is required: "P0" to "P4" if you have judged it, or null for
   * > unassessed if nobody has. Leaving the key out is not a default.
   *
   * ⚠ **A cached bundle of this file older than `5dce9b6` is the one client
   * that really hits it**, which is the reason that message says as much as it
   * does — see `src/wire.rs`.
   */
  // dev-lint: allow-wire-mirror the Rust side is `Ranking`, not `Option<Priority>`, and the rule reads the null arm off the TYPE. `Ranking` is a hand-written Deserialize whose whole purpose is that `null` is legal and ABSENCE is not — the one shape an Option cannot express. Null is right here; the rule cannot see it.
  priority: Priority | null;
  due?: string | null;
  blocked_on?: number[];
  assignee?: Assignee | null;
}

/**
 * A partial change. An absent field means *leave it alone* — which is the whole
 * semantics: a client changing a status must not have to restate a body it has
 * never read.
 */
export interface Change {
  subject?: string | null;
  body?: string | null;
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
  prepend?: string | null;
  /** Text to put below the existing body. The twin of `prepend`; both may be
   *  sent at once, and the same rules apply. */
  append?: string | null;
  status?: Status | null;
  /** Absent means leave it alone, so this cannot UNRANK a task — the same rule
   *  every other field here follows. Ranking it again is the correction. */
  priority?: Priority | null;
  /** The blockers as they should now be — the whole set, not an addition. An
   *  empty array is how a task stops being blocked, which is why there is no
   *  separate unblock flag: `[]` is a value, not an absence. */
  blocked_on?: number[] | null;
  /** Set the day it has to be done by. */
  due?: string | null;
  /** Take it off. A date has no "empty" value the way a blocker list does, so
   *  removing one needs its own field rather than a meaningful null. */
  clear_due?: boolean;
  assignee?: Assignee | null;
  /** Say that a `body` keeping almost nothing of the one it replaces is meant.
   *  Without it the server refuses that write, because it is far more often a
   *  mistake than an edit.
   *
   *  ⚠ **Restoring sets this, and has to.** Putting back what an edit replaced
   *  is a change of subject and body like any other, so undoing an edit that
   *  *grew* a body sends a shorter one — the exact shape the guard refuses. */
  replace_body?: boolean;
}
