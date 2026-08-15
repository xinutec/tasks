-- What one conversation is working on right now, and until when.
--
-- ⚠ **This is the first thing in the schema that HIDES an open task**, so the
-- shape is chosen to make hiding hard to do by accident and impossible to do
-- for ever. Two properties carry that:
--
--   1. **The period is one fact, on one row.** `sessions.focus_until` is the
--      whole of "is this session focused" — clearing is one NULL, and there is
--      no way for two members of a focus set to disagree about when it ends,
--      which a per-row expiry would have allowed.
--   2. **It lapses on its own.** Nothing sweeps this table and nothing needs
--      to: every read compares against NOW(), so a focus nobody cleared stops
--      applying at its hour whether or not the session ever comes back. The
--      expiry IS the reminder, which is the same rule Pippijn mutes fleetwatch
--      checks under.
--
-- The rows are left behind after the period lapses rather than deleted, so
-- `task focus` can say what the last one was; they are replaced wholesale by
-- the next one.
ALTER TABLE sessions
    ADD COLUMN focus_until DATETIME NULL;

-- Which tasks a session's focus period names.
--
-- **A set, not an ordering.** The digest still renders in id order, because
-- focus answers *which* tasks a prompt recites and never *in what order* — two
-- ideas that would fight the moment a rank changed.
--
-- Both foreign keys cascade: a focus on a task that no longer exists is not a
-- focus on anything, and a session's rows have no meaning without the session.
-- This is the one place a deleted task has to take something with it, since a
-- dangling id here would silently shrink a digest rather than fail loudly.
CREATE TABLE IF NOT EXISTS session_focus (
    session VARCHAR(64)     NOT NULL,
    task_id BIGINT UNSIGNED NOT NULL,
    PRIMARY KEY (session, task_id),
    CONSTRAINT fk_session_focus_session FOREIGN KEY (session)
        REFERENCES sessions (id) ON DELETE CASCADE,
    CONSTRAINT fk_session_focus_task FOREIGN KEY (task_id)
        REFERENCES tasks (id) ON DELETE CASCADE
);
