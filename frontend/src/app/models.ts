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

/** Where a task stands. Mirrors `tasks::types::Status`. */
export type Status = 'open' | 'doing' | 'done';

/** Who holds a task. Mirrors `tasks::types::AssigneeKind`. */
export type AssigneeKind = 'nobody' | 'person' | 'session';

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
  repo?: string | null;
  subject: string;
  status: Status;
  assignee: Assignee;
  /** Whether there is prose behind it worth opening. */
  detailed: boolean;
  created_at: string;
  updated_at: string;
  closed_at?: string | null;
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

export interface RepoCount {
  /** `null` is the pile of tasks belonging to no checkout. Always present on
   *  the wire — the backend does not skip it, because a missing key and a task
   *  with no repository would then look the same. */
  repo: string | null;
  open: number;
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
 * one place the mirror runs backwards. Omitting `repo` is what "no repository"
 * means and omitting `assignee` is what "leave it in the pile" means, so both
 * are optional — see the note on `NewTask` in `tasks/repo.rs` for how that is
 * stated on the other side.
 */
export interface NewTask {
  repo?: string | null;
  subject: string;
  body: string;
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
  assignee?: Assignee | null;
}
