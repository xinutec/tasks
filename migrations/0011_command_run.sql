-- What the CLI actually did, as opposed to what a probe imagined it doing.
--
-- `check_run` (0010) records the two model checks. It answers "what does a
-- filing wait for" and nothing else: every other command — `list`, `show`,
-- `edit`, `done`, `digest` — is unmeasured, and those are what a session runs
-- all day. A tracker that had become slow to read would have surfaced as
-- conversations feeling sluggish and as nothing written down anywhere.
--
-- The first attempt at this was a fleetwatch collector on a 15-minute launchd
-- timer, timing `task list --all` from outside. Pippijn refused that shape on
-- 2026-08-25 — "don't do synthetic stuff, measure what's actually going on" —
-- and he is right twice over. It timed a command nobody runs, from a process
-- with a cold cache and no session, and it reported those numbers as though
-- they were what sessions experience. A poll also cannot see the distribution:
-- it takes one sample every 900 seconds whether the tool was used a hundred
-- times in that window or not at all.
--
-- ⚠ **No foreign keys, deliberately** — the same reasoning as `check_run`. This
-- measures the tool. It must outlive the tasks it names, and a row must never be
-- refused because a task was deleted or because `task_id` names something that
-- was never created (a filing that was refused has no id at all).
--
-- ⚠ **`verb` is a fixed vocabulary, not free text.** It is the trend key: a
-- reader groups by it, and a value that arrives spelled two ways silently splits
-- one command's history in half. The CLI supplies it from an exhaustive match on
-- its own command enum, so adding a subcommand without naming it here is a
-- compile error rather than a gap in the data.
CREATE TABLE command_run (
    id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    ran_at      DATETIME        NOT NULL,
    -- `list`, `show`, `add`, `edit`, `done`, `digest`, …
    verb        VARCHAR(32)     NOT NULL,
    -- The conversation that ran it, so a slow command can be told apart from a
    -- slow machine: several sessions share this Mac.
    session     VARCHAR(64)             ,
    -- Wall clock for the whole command as the caller experienced it, including
    -- the round trips it made and everything it printed.
    elapsed_ms  INT UNSIGNED    NOT NULL,
    -- `ok` or `error`. A command that failed is still a command that took time,
    -- and the two must be countable apart: an error path is usually the fast one
    -- and folding them together would report the tool as quicker than it is.
    outcome     VARCHAR(16)     NOT NULL,
    INDEX idx_command_run_verb (verb, ran_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
