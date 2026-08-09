-- The last piece of the file scheme, retired.
--
-- `0001_init.sql` gave a task a `repo` because the thing this service replaced
-- was a `TASKS.md` per checkout, and the README said plainly what that column
-- was: filtering on repository alone "was inherited rather than chosen — one
-- `TASKS.md` per repo put both parties' work in one file because there was
-- nowhere else to put it". A database has somewhere else to put it. The holder
-- is the answer to *whose work is this*, and it has been since `0001`.
--
-- ⚠ **A Claude session is routinely wider than one checkout.** Fleet work spans
-- `xinutec-infra` and `nixos-config`; this session held tasks across `dev-lint`,
-- `utterance` and `~/.claude`. Assigning per repo asked a question with no
-- single answer and then used the answer to decide what reached a prompt.
--
-- What made it worse than redundant: the digest selected on *claimed* repos, so
-- a session that had claimed nothing got an EMPTY digest — indistinguishable
-- from a failed migration. And a task moved to the right holder in the wrong
-- repo became invisible to the receiver. #9 did exactly that on 2026-08-09,
-- moved `dev-lint -> health` while still tagged `dev-lint`, and `task edit` had
-- no `--repo` with which to correct it.
--
-- The pile is now global rather than per-repo, which is the one behavioural
-- change: with no repo there is no scope to narrow it to. Measured before
-- dropping — 3 unheld open tasks out of 134 — so the digest grows by three
-- lines for the sessions that were seeing none of them, and the handover
-- channel the README insists on stays open.
--
-- ⚠ **Spent before dropped**, the same care `0003` took. For most tasks the
-- checkout is already in the subject or the body, but not for all of them, and
-- a column deleted outright would take the rest with it silently. So every
-- non-null repo is written into `task_events` first — which is the table that
-- exists to be the history git used to keep, and precisely where "this was
-- tagged `mac-config`" belongs once the column is gone. `task show <id>` prints
-- it, so nothing becomes unanswerable.
INSERT INTO task_events (task_id, actor_kind, actor_id, kind, detail)
SELECT id, 'person', 'pippijn', 'edited', CONCAT('was in repo ', repo, '; repo retired')
FROM tasks
WHERE repo IS NOT NULL;

-- The digest's old query. `idx_tasks_session` is the one that survives, because
-- "what is this session holding" is now the whole of the question.
DROP INDEX idx_tasks_repo_status ON tasks;

ALTER TABLE tasks
    DROP COLUMN repo;
