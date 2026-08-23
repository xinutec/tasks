-- What a model check did: one row per run.
--
-- ⚠ **Neither check could be counted before this.** The filing check's only
-- trace was a line on the caller's stderr, which survives just as long as that
-- session's transcript — 5 timeouts were recoverable that way against 280 tasks
-- filed since 2026-08-14, a floor rather than a rate. The density read leaves
-- nothing at all, and on purpose: it runs after the edit has landed, so
-- `accreting()` swallows every failure rather than spend a session's attention
-- on the checker. Neither the firing rate nor the latency distribution was a
-- fact about this tool; both were guesses from whatever a grep happened to find.
--
-- **No foreign keys, deliberately.** This measures the tool, not a task's
-- history: a row has to outlive its subject, and must never be refused because
-- a `sessions` row has not been written yet. That is the opposite of
-- `task_revision`, which is one task's recovery path and cascades with it.
CREATE TABLE IF NOT EXISTS check_run (
    id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    ran_at      DATETIME        NOT NULL,
    -- `filing`, before a task exists, against every open title; or `density`,
    -- after an edit, against one body.
    kind        VARCHAR(16)     NOT NULL,
    -- Who paid the latency.
    session     VARCHAR(64)     NOT NULL,
    -- The body that was read. NULL on `filing`: what it would have named does
    -- not exist while the check is running, which is the point of running it
    -- there.
    task_id     BIGINT UNSIGNED NULL,
    -- Characters put to the model. The one number that means the same thing for
    -- a corpus of titles and for a single body.
    input_chars INT UNSIGNED    NOT NULL,
    -- What crossed the sampler. `density` only.
    accreted    INT UNSIGNED    NULL,
    elapsed_ms  INT UNSIGNED    NOT NULL,
    -- `quiet` (ran, said nothing), `spoke` (named a duplicate, or gave advice),
    -- `timeout`, `error`.
    outcome     VARCHAR(16)     NOT NULL,
    -- Both questions are asked per kind over a period: how often does it fire,
    -- and how long does it take.
    INDEX idx_check_run_kind (kind, ran_at)
);
