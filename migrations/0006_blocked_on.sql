-- What a task is waiting for, as tasks rather than as a sentence.
--
-- Pippijn, 2026-08-11: *"If something is blocked, it should get a ticket number
-- that it's blocked on. It can be the same, but not higher priority than the
-- thing it's blocked on."* Two things in one rule — a link, and a constraint on
-- the ranks at both ends of it.
--
-- ⚠ **Prose already carried this and prose goes stale.** Twelve open tasks named
-- a blocker in their body when this was added, in six spellings (`blocked on`,
-- `BLOCKED on`, `blockedBy:`, `blocked until`, `gated on`, `waiting on`). **Five
-- of the twelve named a blocker that was already closed** — #128 on #125, #424
-- on #427, #450 on #614, #743 on #741 — so a reader who believed the body would
-- think the task was stuck when it was ready to start. A join to the live row
-- cannot say that.
--
-- ⚠ **A TABLE rather than a column, and the first attempt got this wrong.** The
-- measurement said none of those twelve named more than one blocker, and that
-- was read as "one is enough". It is not evidence: nobody recorded two blockers
-- because there was nowhere to record one, so the count measures the absence of
-- this feature rather than the shape of the work. Pippijn caught it. A single
-- column would have made the workaround for a second blocker *"put it in the
-- body"* — reintroducing the staleness above, for exactly the tasks with the
-- most dependencies.
--
-- ⚠ **`ON DELETE CASCADE` here is right and would be wrong on `tasks`.** These
-- rows are edges, not records: a task's edges have no meaning without it.
-- Nothing deletes a task — closing is `done` or `dropped` and the row stays — so
-- this is a backstop rather than a path.
--
-- The rank constraint is NOT expressed here. A MariaDB `CHECK` cannot reach
-- another row, and with several blockers the rule is *no more urgent than the
-- LEAST urgent open one*. It lives in `repo::blocking_is_consistent`, which can
-- name the offending task in the refusal — which is the point of refusing.
CREATE TABLE IF NOT EXISTS task_blocks (
    -- The task that is waiting.
    task_id    BIGINT UNSIGNED NOT NULL,
    -- The task it is waiting for.
    blocked_on BIGINT UNSIGNED NOT NULL,

    PRIMARY KEY (task_id, blocked_on),
    -- "What is #726 waiting for" is the read every list does; the primary key
    -- serves it. This one serves the other direction — "what is waiting on
    -- #697" — which the rank rule has to ask on every re-rank.
    INDEX idx_task_blocks_blocked_on (blocked_on),

    CONSTRAINT fk_task_blocks_task FOREIGN KEY (task_id)
        REFERENCES tasks (id) ON DELETE CASCADE,
    CONSTRAINT fk_task_blocks_blocked_on FOREIGN KEY (blocked_on)
        REFERENCES tasks (id) ON DELETE CASCADE
);
