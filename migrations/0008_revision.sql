-- What an edit replaced, so that overwriting a body is not final.
--
-- ⚠ **The loss this answers happened, and the schema was the reason it was
-- expensive.** 2026-08-14: a session rewrote #25's body from a snapshot it had
-- read three days earlier and never re-read. `task_events` recorded *that* the
-- body changed — `detail` is VARCHAR(512) and holds one rendered line, `body` —
-- so nothing in the service knew what had been there. It was recovered only
-- because the writer's transcript still had the heredoc, which depends on the
-- writer being a session, on its transcript surviving, and on somebody thinking
-- to grep. That is luck, not a recovery path.
--
-- ⚠ **A snapshot of both columns, not a diff and not one column.** A subject
-- edit and a body edit are two `task_events` rows written by one update, so a
-- per-event record of "what this one replaced" would leave `task undo` guessing
-- which rows belonged together. Storing the whole prior text of both, once per
-- update, makes a revision a complete previous version of the task: restoring
-- it needs no reassembly and cannot half-apply. The duplication when only the
-- subject moved is a few kB against a body column that already exists.
--
-- **Keyed to the event rather than carrying its own actor and time.** The
-- event already says who and when, and this table is read by joining to it. Two
-- copies of an actor is the pair that drifts. The FK is to the FIRST `edited`
-- event of the update, so `ORDER BY event_id DESC` is newest-first.
--
-- Retention needs no policy: `ON DELETE CASCADE` twice over means a revision
-- lives exactly as long as the event it belongs to, which lives exactly as long
-- as the task.
CREATE TABLE IF NOT EXISTS task_revision (
    event_id BIGINT UNSIGNED NOT NULL PRIMARY KEY,
    -- The subject and body as they stood immediately BEFORE the event this row
    -- is keyed to. Same types as `tasks`, because that is what they came from
    -- and what they go back to.
    subject  VARCHAR(200) NOT NULL,
    body     MEDIUMTEXT   NOT NULL,
    CONSTRAINT fk_task_revision_event FOREIGN KEY (event_id)
        REFERENCES task_events (id) ON DELETE CASCADE
);
