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
}
