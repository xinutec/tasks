-- Which tasks a filing check named.
--
-- A refusal is only judgeable by what it matched: whether to reopen that task
-- or override the check. Kept here, the answer survives a caller that read only
-- the tail of the refusal, and `task checks` can show it afterwards.
--
-- A row per id rather than a joined string. No foreign keys, for the reason
-- `check_run` (0010) gives: a row must outlive the tasks it names.
CREATE TABLE check_match (
    run_id  BIGINT UNSIGNED NOT NULL,
    task_id BIGINT UNSIGNED NOT NULL,
    PRIMARY KEY (run_id, task_id)
);
