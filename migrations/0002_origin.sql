-- Where an imported task came from, and what it was called there.
--
-- 598 tasks were migrated out of the CLI's built-in per-session stores
-- (`~/.claude/tasks/<session-id>/<n>.json`) on 2026-08-08. Those numbers are
-- unique only *within* a session, and this service has one global id space: 124
-- numbers were claimed by more than one session and `#77`, `#78`, `#79` and
-- `#21` by four each, so 46% of them could not keep the number they had.
--
-- ⚠ **The policy is "keep the number where it is free", which without these two
-- columns would be the worst of both worlds** — some ids meaning the old thing,
-- some not, and no way to tell which. That was the stated objection to it. These
-- columns answer it: every imported task says where it came from, so a preserved
-- number can be *verified* rather than assumed, and a renumbered one can still be
-- found by what it used to be called.
--
-- Open tasks were given priority for their original number, because those are
-- the ones anybody will refer to; a finished task's number is only ever read
-- backwards, out of prose that also names its session.
ALTER TABLE tasks
    -- The session that owned it, by NAME (`health`, `life`) rather than by uuid:
    -- this is read by a person next to the number, and `health#79` is a name
    -- while `296dae53…#79` is a fingerprint. The uuid is not lost — it is on the
    -- `sessions` row this task's assignee points at.
    ADD COLUMN origin_session VARCHAR(128) NULL,
    -- What it was numbered there. NULL for a task filed here, which is most of
    -- them from now on: absence means "born in this service", not "unknown".
    ADD COLUMN origin_number INT UNSIGNED NULL;

-- Finding a task by what it used to be called is the whole point of the two
-- columns, and it is how old prose citing `#79` gets resolved.
CREATE INDEX idx_tasks_origin ON tasks (origin_session, origin_number);
