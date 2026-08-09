-- The import's two columns, retired once the mapping had been spent.
--
-- `0002_origin.sql` added `origin_session` / `origin_number` so that a session
-- remembering `recall#79` could still find the task it became: 620 tasks came
-- out of the per-session stores on 2026-08-08 and 178 of them could not keep
-- their number. They were transitional by design, and Pippijn confirmed on
-- 2026-08-09 that every session had moved.
--
-- ⚠ **Spent before dropped, which is the whole of the care here.** A mapping
-- deleted while prose still cites the old space turns that prose into dead
-- references, silently — the same shape of failure as everything else in this
-- service that goes wrong without anything erroring. So, first:
--
--   * 29 tasks carried machine-written `blockedBy` / `blocks` footers in the
--     old space (the task filed for this said 15 — it was counted by hand).
--     Every one of the references resolved exactly, and they were rewritten to
--     live ids. `#122 blocks #452` is the case that shows why it mattered: read
--     naively the footer said `125`, which is a different task.
--   * 21 more citations sat in ordinary prose across 15 tasks. Each was checked
--     against its target's subject before rewriting — "in-memory result cache
--     (#125)" against *In-memory cache for /api/velocity results* — rather than
--     substituted on pattern alone. 449 further `#N` mentions resolved to the
--     same id either way and were left untouched.
--   * Nothing outside the service depended on it: no memory file under
--     `~/.claude/projects/*/memory/` cites the old space, and a scan of all 25
--     repositories under `~/Code` found only `tasks#636` in two memview
--     comments — this service's own live id wearing the old spelling, which
--     still reads correctly as `#636`.
--
-- What survives is the `#79` spelling of a plain id, which is what the digest
-- prints on every line of every prompt. What goes is the second id space.
DROP INDEX idx_tasks_origin ON tasks;

ALTER TABLE tasks
    DROP COLUMN origin_session,
    DROP COLUMN origin_number;
