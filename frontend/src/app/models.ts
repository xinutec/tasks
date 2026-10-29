/**
 * What the backend sends, restated by hand. `Record<Status, …>` in `holder.ts`
 * makes a missed vocabulary member a build error here; dev-lint's
 * `DL-WIRE-MIRROR-DRIFT` fails when a field's optionality or nullability
 * disagrees with Rust: `?` mirrors `skip_serializing_if`, `| null` mirrors
 * `Option`.
 */

/**
 * Mirrors `tasks::types::Status`. `dropped` is closed without being done, so nobody is credited.
 */
export type Status = 'open' | 'doing' | 'done' | 'dropped';

/** Who holds a task. Mirrors `tasks::types::AssigneeKind`. */
export type AssigneeKind = 'nobody' | 'person' | 'session';

/**
 * Mirrors `tasks::types::Priority`. Absence is not a member and must not get
 * one: most tasks are unranked and sort as `P2`. The backend sorts; nothing here
 * reorders.
 */
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
  /** The deadline, `YYYY-MM-DD`; usually absent. Don't sort by it here (see `escalated_to`). */
  due?: string;
  /**
   * What it sorts as when a near deadline raised it: `'P0'` or absent. Draw
   * `escalated_to ?? priority`, never recompute the rule; `priority` still holds
   * what was chosen.
   */
  escalated_to?: Priority;
  /** Whether `due` has passed, by the server's clock; never recompute it here (timezones). */
  overdue?: boolean;
  /**
   * What this waits for, oldest first; absent when none. Kept after a blocker
   * closes, so it isn't `blocked`, which the backend answers.
   */
  blocked_on?: number[];
  /** Whether any blocker is still open. Absent means false. */
  blocked?: boolean;
  assignee: Assignee;
  /** Whether there is prose behind it worth opening. */
  detailed: boolean;
  /** Body lines, for the CLI; no view draws it. */
  body_lines: number;
  /**
   * What the filing session calls itself; absent when not said. Drawn only where there is
   * no holder.
   */
  filed_by?: string;
  /**
   * The body's length when a model last read it and spoke; absent nearly always.
   * The words are on the detail (`sprawl_said`); draw this as a mark.
   */
  sprawl_chars?: number;
  created_at: string;
  updated_at: string;
  closed_at?: string;
}

/**
 * A task just filed, and the closed tasks it names that the filer closed in the last day
 * (`tasks::lifecycle`).
 */
export interface Created extends Task {
  /** Absent, not empty, when it names none. */
  closed?: Closed[];
}

export interface Closed {
  id: number;
  status: Status;
  /** When the filer closed it. */
  at: string;
}

/** A task after a write; `changed` lists the event kinds written, empty for a no-op. */
export interface Updated extends Task {
  changed: string[];
  /** What this write overwrote; absent unless it moved prose. */
  replaced?: Replaced;
  /**
   * When the text was last written, if this closed the task without a rewrite
   * (`tasks::lifecycle::unwritten`).
   */
  unwritten?: string;
}

/**
 * What an edit landed on, shown at the write, refusing nothing
 * (`tasks::types::Replaced`). Undo: `GET /api/tasks/{id}/previous` and write back.
 */
export interface Replaced {
  at: string;
  by: string;
  /** Body length before and after, in characters. */
  was: number;
  now: number;
  /** Growth since the body last shrank, this edit included; zero on a consolidating edit. */
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
  /** Whether there is a previous version, without fetching it. */
  restorable: boolean;
  /** What a model last said about this body, kept for whoever opens it. */
  sprawl_said?: string;
}

/** A task as it stood before its most recent edit — `GET /api/tasks/{id}/previous`. */
export interface Revision {
  at: string;
  actor: string;
  /**
   * Whether the edit this would revert is the asker's. Restoring reverts the
   * task's last edit; the CLI refuses on this, while this page shows
   * `Replaced <when> by <actor>` above the buttons instead.
   */
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
 * One party's share, as `/api/holders` answers; `total` adds finished work, so a cleared
 * plate isn't idle.
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
 * A task being filed, a request body (the mirror runs backwards). Omitting
 * `assignee` files it to the filer, not the pile.
 */
export interface NewTask {
  subject: string;
  body: string;
  /**
   * Whether the duplicate check ran. Always sent; the web UI can't skip the
   * check, so it says `true`.
   */
  checked: boolean;
  /** The tasks an override judged different; the web UI never sends it. */
  not_duplicate_of?: number[];
  /**
   * The one required field (no `?`): a level, or `null` for unassessed. Both
   * sort as `P2`, but `P2` claims somebody looked. Omitting it is a 400, not a
   * default (`src/wire.rs`).
   */
  priority: Priority | null;
  due?: string;
  blocked_on?: number[];
  assignee?: Assignee;
  /**
   * Why this is nobody's: required with a `nobody` assignee and refused
   * otherwise (a 400 either way); whitespace is no answer.
   */
  spare?: string;
}

/** A partial change: absent means leave it alone, so a status change needn't restate the body. */
export interface Change {
  subject?: string;
  body?: string;
  /**
   * Text added above the body, keeping all of it, resolved server-side under the
   * row lock. Never read-concatenate-PATCH here: two clients would lose an
   * addition. With `body` it is a 400.
   */
  prepend?: string;
  /** Text added below the body; may be sent with `prepend`. */
  append?: string;
  status?: Status;
  /** Can't unrank; rank again to correct. */
  priority?: Priority;
  /** The whole blocker set; `[]` unblocks. */
  blocked_on?: number[];
  /** Set the day it has to be done by. */
  due?: string;
  /** Take the deadline off: a date has no empty value, so it needs its own field. */
  clear_due?: boolean;
  assignee?: Assignee;
  /**
   * Says a `body` keeping almost nothing of the old one is meant; otherwise the
   * server refuses it. Restoring sets this: undoing an edit that grew a body sends
   * a shorter one.
   */
  replace_body?: boolean;
}
