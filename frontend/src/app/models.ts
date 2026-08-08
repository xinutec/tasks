/**
 * What the backend sends.
 *
 * ⚠ **Restated by hand, not generated.** The vocabularies here — `Status` and
 * `AssigneeKind` — are closed sets the Rust side also enumerates, and nothing
 * links the two at compile time. `Record<Status, …>` in the views is what turns
 * a missed member into a build error on this side; the backend's own
 * exhaustive `match` does the same over there. Adding a member is a two-file
 * change and both files refuse to be forgotten.
 */

/** Where a task stands. Mirrors `tasks::types::Status`. */
export type Status = 'open' | 'doing' | 'done';

/** Who holds a task. Mirrors `tasks::types::AssigneeKind`. */
export type AssigneeKind = 'nobody' | 'person' | 'session';

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
  repo?: string;
  subject: string;
  status: Status;
  assignee: Assignee;
  /** Whether there is prose behind it worth opening. */
  detailed: boolean;
  created_at: string;
  updated_at: string;
  closed_at?: string;
}

export interface TaskEvent {
  at: string;
  kind: string;
  detail?: string;
  actor: string;
}

export interface TaskDetail extends Task {
  body: string;
  body_html: string;
  events: TaskEvent[];
}

export interface Session {
  id: string;
  name?: string;
  first_seen: string;
  last_seen: string;
  open: number;
}

export interface RepoCount {
  repo?: string;
  open: number;
}

/** Who the caller is, as `/api/me` answers. */
export interface Me {
  kind: 'person' | 'session';
  id: string;
  name?: string;
}

export interface NewTask {
  repo?: string;
  subject: string;
  body: string;
  assignee?: Assignee;
}

/** A partial change. An absent field means *leave it alone*. */
export interface Change {
  subject?: string;
  body?: string;
  status?: Status;
  assignee?: Assignee;
}
