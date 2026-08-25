-- When the timings were last carried out to fleetwatch, and who is carrying
-- them next.
--
-- ⚠ **This row IS the lock.** Thirteen conversations share this Mac and any of
-- them may be the one to send. A stamp in a file on the caller's disk would let
-- five sessions read "due" in the same second and push five reports; a row the
-- service claims with a conditional UPDATE has exactly one winner, and the
-- loser learns it lost from the same response it was already getting.
--
-- ⚠ **Claimed on the way out, not on the way back.** The row is stamped when a
-- caller is TOLD to report, not when the report lands. A caller that is handed
-- the job and then dies — no network, killed terminal — costs one skipped
-- window and nothing else. Stamping on success instead would mean a caller that
-- crashes leaves the claim open, and the next command claims it again, and a
-- fleetwatch outage turns into every session retrying the push.
--
-- One row, ever. `what` names the thing being reported so a second consumer
-- does not need a second table.
CREATE TABLE reported (
    what     VARCHAR(32) NOT NULL PRIMARY KEY,
    -- When a caller was last handed the job.
    claimed_at DATETIME  NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
