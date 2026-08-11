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
  priority?: Priority | null;
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
  assignee?: Assignee | null;
}
