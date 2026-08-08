-- The work itself, and who is holding it.
--
-- Three tables, and the shape of them follows from what this service replaced.
-- Tasks used to be `TASKS.md` (one line per open task) plus a body file per
-- task, committed in each repo, and *finished tasks were deleted* — because
-- keeping them is what turned 48 live items into 366, and git recorded what was
-- done better than a status flag did.
--
-- ⚠ **With no files there is no git, so this schema has to be the record git
-- was.** Hence: a finished task is KEPT (`status = 'done'`), never deleted, and
-- `task_events` holds the history of who moved what. What preserves the original
-- property is not deletion but the query — nothing that reaches a prompt ever
-- selects a done row. See `digest.rs`.

-- A Claude Code conversation.
--
-- ⚠ **The CLI's session id is the identity, and the name is an attribute.** A
-- session renames itself as its job changes and pushes the new name here; that
-- is an UPDATE of one column, and every task assigned to it stays assigned.
-- Making the name the key would have re-pointed the whole list on a rename.
CREATE TABLE IF NOT EXISTS sessions (
    id         VARCHAR(64)  NOT NULL PRIMARY KEY,
    -- Nullable because a session may claim a list before it has named itself;
    -- an empty name would read as a session called "".
    name       VARCHAR(255) NULL,
    first_seen DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen  DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_sessions_name (name)
);

-- One task.
--
-- `id` is global and is what everything displays. The file scheme numbered per
-- repo, and its own hook documented the cost: "a bare #4 means nothing when two
-- repos both have one". One id space means `task show 4` needs no repo, and the
-- digest can still group by repo for a reader.
CREATE TABLE IF NOT EXISTS tasks (
    id      BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    -- Which checkout the work is in, as a bare directory name (`memview`), not
    -- a path: a path is machine-specific and the sessions that claim these are
    -- not the only readers. NULL is a task that belongs to no repo — most of
    -- Pippijn's own, which had nowhere to live under the file scheme at all.
    repo    VARCHAR(128) NULL,
    -- The whole of what is ever injected into a prompt. Length-capped on
    -- purpose: this column IS the per-turn cost, and a subject that does not fit
    -- in a line is a body.
    subject VARCHAR(200) NOT NULL,
    -- Markdown, fetched only when a task is opened. Never injected.
    body    MEDIUMTEXT   NOT NULL,
    -- open | doing | done. VARCHAR rather than ENUM: the Rust side hand-writes
    -- Type/Encode/Decode delegating to `str`, and a derived `sqlx::Type` would
    -- declare the SQL type as ENUM and fail every read of a VARCHAR column.
    status  VARCHAR(16)  NOT NULL DEFAULT 'open',

    -- Who is holding it. Three states, so two columns rather than one nullable
    -- name: `nobody` (in the repo's pile, whoever picks it up), `person` (a
    -- Nextcloud user id), `session` (a conversation).
    assignee_kind    VARCHAR(16) NOT NULL DEFAULT 'nobody',
    assignee_person  VARCHAR(255) NULL,
    assignee_session VARCHAR(64)  NULL,

    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    -- When it stopped being open. Kept apart from `updated_at`, which moves for
    -- an edit to the body long after.
    closed_at  DATETIME NULL,

    -- The digest's query: open rows for a repo, in id order.
    INDEX idx_tasks_repo_status (repo, status),
    -- The digest's other query: what one session is holding, across repos.
    INDEX idx_tasks_session (assignee_session, status),
    CONSTRAINT fk_tasks_session FOREIGN KEY (assignee_session)
        REFERENCES sessions (id) ON DELETE SET NULL
);

-- What happened to a task, and who did it.
--
-- This is the half of git that mattered: a task moving between Pippijn and a
-- session is the thing this service exists to support, and a schema that kept
-- only the current assignee could not answer "who has had this" at all.
CREATE TABLE IF NOT EXISTS task_events (
    id         BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    task_id    BIGINT UNSIGNED NOT NULL,
    at         DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- Who made the change: `person` or `session`, with the id of whichever.
    actor_kind VARCHAR(16)  NOT NULL,
    actor_id   VARCHAR(255) NULL,
    -- created | status | assigned | edited
    kind       VARCHAR(24)  NOT NULL,
    -- One line, already rendered: `open → doing`, `pippijn → memview`. Rendered
    -- at write time because the ids it names may be gone by the time anybody
    -- reads it, and a history that stops resolving is not a history.
    detail     VARCHAR(512) NULL,
    INDEX idx_task_events_task (task_id, at),
    CONSTRAINT fk_task_events_task FOREIGN KEY (task_id)
        REFERENCES tasks (id) ON DELETE CASCADE
);
