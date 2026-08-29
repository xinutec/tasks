//! What the CLI recorded about itself.
//!
//! ⚠ **Every row this models is a command somebody actually ran.** The first
//! version of this measurement was a launchd timer running `task list --all`
//! every 15 minutes — a command no session runs, from a process with no session
//! id and a cold cache — and reporting those numbers as latency. Pippijn refused
//! that shape on 2026-08-25: measure what is actually going on, and do not poll.

use tasks::tasks::commands::{Ended, Ran, Tally, tally};

fn ran(verb: &str, ms: u32, outcome: Ended) -> Ran {
    Ran {
        ran_at: chrono::Utc::now(),
        verb: verb.to_string(),
        elapsed_ms: ms,
        outcome,
        // The fixtures below are about the verb split; `waited` has its own
        // module at the foot of this file, which builds rows that say.
        waited_for_a_model: None,
    }
}

/// The same row, saying whether it waited for a model.
fn ran_waiting(verb: &str, ms: u32, waited: bool) -> Ran {
    Ran {
        waited_for_a_model: Some(waited),
        ..ran(verb, ms, Ended::Ok)
    }
}

/// ⚠ **The error path is usually the FAST one.** A refusal prints and returns
/// without a round trip, so folding it into the percentiles reports the tool as
/// quicker than any session experiences it — and fastest on the day it starts
/// refusing everything. `failed` carries that half beside the timings instead.
#[test]
fn a_failed_run_is_counted_but_never_timed() {
    // ⚠ TWO refusals, not one, and that is what makes this test discriminate.
    // With a single 40 ms error among two successes the median lands on 9,000
    // either way, so the assertion passed with the guard deleted — a check that
    // can pass for the wrong reason silences the question it was asked. Two
    // errors move the unfiltered median to 50 ms, which is the failure to catch.
    let runs = vec![
        ran("add", 9_000, Ended::Ok),
        ran("add", 11_000, Ended::Ok),
        ran("add", 40, Ended::Error),
        ran("add", 50, Ended::Error),
    ];
    let line = &tally(&runs)[0];
    assert_eq!(line.runs, 4, "every run is counted");
    assert_eq!(line.failed, 2);
    assert_eq!(
        line.median_ms, 9_000,
        "the refusals are counted, never timed: unfiltered this reads 50 ms"
    );
    assert_eq!(line.worst_ms, 11_000);
}

/// ⚠ **Busiest first, because the run count is the weight.** A command run four
/// times with a bad worst case matters less than the one every session runs all
/// day being slower.
#[test]
fn the_busiest_command_leads() {
    let mut runs = vec![ran("edit", 60_000, Ended::Ok)];
    for _ in 0..5 {
        runs.push(ran("list", 300, Ended::Ok));
    }
    let lines = tally(&runs);
    assert_eq!(
        lines[0].verb, "list",
        "five runs of 300ms outrank one of a minute"
    );
    assert_eq!(lines[1].verb, "edit");
}

/// Two verbs run equally often keep a stable order, or a diff of two days is
/// unreadable for a reason that has nothing to do with the tracker.
#[test]
fn an_equal_tally_is_ordered_by_name() {
    let runs = vec![
        ran("show", 100, Ended::Ok),
        ran("done", 100, Ended::Ok),
        ran("list", 100, Ended::Ok),
    ];
    let lines = tally(&runs);
    let order: Vec<&str> = lines.iter().map(|t| t.verb.as_str()).collect();
    assert_eq!(order, vec!["done", "list", "show"]);
}

/// A verb whose every run failed still appears — with no timings and the count
/// of what went wrong, which is the whole finding.
#[test]
fn a_command_that_only_ever_failed_is_still_reported() {
    let runs = vec![ran("move", 50, Ended::Error), ran("move", 60, Ended::Error)];
    assert_eq!(
        tally(&runs),
        vec![Tally {
            verb: "move".into(),
            runs: 2,
            failed: 2,
            refused: 0,
            median_ms: 0,
            p90_ms: 0,
            worst_ms: 0,
            unchecked_p90_ms: None,
            waited: 0,
            unknown: 2,
        }]
    );
}

/// ⚠ **Five days is the requirement, and the constant is worked back from
/// fleetwatch's bands rather than picked.** It grades a report `Fresh` within
/// 1.5× the declared interval, `Overdue` to 3×, and `Silent` — rendered as a
/// FAILURE — beyond. Pippijn, 2026-08-25: five days of nothing is a problem,
/// anything short of it is not. So 3× must land exactly on five days, and a
/// normal quiet weekend must stay inside `Fresh`.
#[test]
fn the_declared_interval_puts_the_failure_at_five_days() {
    use tasks::tasks::commands::REPORTING_INTERVAL_S;
    let day = 86_400;
    assert_eq!(
        REPORTING_INTERVAL_S * 3,
        5 * day,
        "silence becomes a failure at five days, not sooner"
    );
    assert!(
        REPORTING_INTERVAL_S * 3 / 2 >= 2 * day,
        "a quiet weekend must not even warn"
    );
}

/// Splitting the latency by the variable that actually explains it.
///
/// ⚠ **`edit p90` was reporting the CHECK RATE.** Measured over the four days to
/// 2026-08-29, aligned so the two tables cover the same window, slow edits and
/// density reads are 1:1 — 161 and 161. An unchecked edit ran 235 ms at the
/// median, a checked one 39,351 ms, and the service's own share of the checked
/// one was ~337 ms: the same flat cost. So the one reported figure of 58,415 ms
/// described neither population. It moves when the check rate moves and when the
/// model slows down, and cannot say which happened.
mod waited {
    use super::*;

    #[test]
    fn the_service_is_reported_apart_from_the_model_it_waited_for() {
        // The real shape, in miniature: a fast majority and a slow checked tail.
        let mut runs: Vec<Ran> = (0..7).map(|_| ran_waiting("edit", 240, false)).collect();
        runs.push(ran_waiting("edit", 39_000, true));
        runs.push(ran_waiting("edit", 90_000, true));
        let out = tally(&runs);
        let edit = &out[0];

        assert_eq!(edit.runs, 9);
        assert_eq!(edit.waited, 2);
        assert!(
            edit.p90_ms > 30_000,
            "the mix still carries the model: {}",
            edit.p90_ms
        );
        assert_eq!(
            edit.unchecked_p90_ms,
            Some(240),
            "the service's own latency is what a regression would show in"
        );
    }

    #[test]
    fn a_run_that_never_said_is_counted_as_unknown_and_not_as_fast() {
        // ⚠ Rows written before `0015` know nothing. Folding them into the
        // unchecked population would file 39-second edits as 235 ms ones —
        // inventing the very number the split exists to measure.
        let runs = vec![
            ran("edit", 39_000, Ended::Ok),
            ran("edit", 90_000, Ended::Ok),
            ran_waiting("edit", 240, false),
        ];
        let edit = &tally(&runs)[0];
        assert_eq!(edit.unknown, 2);
        assert_eq!(edit.waited, 0);
        assert_eq!(
            edit.unchecked_p90_ms,
            Some(240),
            "an unknown row was counted as unchecked"
        );
    }

    #[test]
    fn a_verb_nobody_said_anything_about_reports_no_split() {
        // Absent, not equal to the mix. A figure that quietly falls back to the
        // number it corrects looks exactly like the correction working.
        let runs = vec![
            ran("edit", 39_000, Ended::Ok),
            ran("edit", 90_000, Ended::Ok),
        ];
        assert_eq!(tally(&runs)[0].unchecked_p90_ms, None);
    }

    #[test]
    fn a_failed_run_stays_out_of_both_timings() {
        // The existing rule: a refusal returns without a round trip, so folding
        // the error path in makes the tool look fastest when it refuses most.
        let runs = vec![
            ran_waiting("add", 12_000, true),
            Ran {
                waited_for_a_model: Some(false),
                ..ran("add", 30, Ended::Error)
            },
        ];
        let add = &tally(&runs)[0];
        assert_eq!(add.failed, 1);
        assert_eq!(
            add.unchecked_p90_ms, None,
            "a failed run became the service's latency"
        );
    }
}

/// A refusal is the tool working, and it used to be counted as breakage.
///
/// ⚠ **`add` ended badly on 149 of 272 runs, which reads as a broken command.**
/// Split by how long they took, 76 ended in **0-14 ms** — a round trip costs
/// ~200 ms, so those never reached the service: they are the CLI declining a
/// malformed invocation. The rest took 5-20 s and are the duplicate check
/// refusing. Both are the tool doing its job, and one number was carrying them
/// and any real fault together.
mod declined {
    use super::*;

    fn refused(verb: &str, ms: u32) -> Ran {
        Ran {
            waited_for_a_model: Some(false),
            ..ran(verb, ms, Ended::Refused)
        }
    }

    #[test]
    fn a_refusal_is_counted_apart_from_a_fault() {
        let runs = vec![
            ran_waiting("add", 12_000, true),
            refused("add", 3),
            refused("add", 5),
            ran("add", 40, Ended::Error),
        ];
        let add = &tally(&runs)[0];
        assert_eq!(add.runs, 4);
        assert_eq!(add.refused, 2, "the guards that fired");
        assert_eq!(add.failed, 1, "the one thing that actually went wrong");
    }

    #[test]
    fn a_refusal_is_never_timed_any_more_than_a_fault_is() {
        // The existing rule, which the new arm must not slip past: a refusal
        // returns without a round trip, so folding it into the percentiles makes
        // the tool look fastest on the day it refuses everything.
        let runs = vec![ran_waiting("add", 12_000, true), refused("add", 3)];
        let add = &tally(&runs)[0];
        assert_eq!(add.median_ms, 12_000, "a 3 ms refusal entered the timings");
        assert_eq!(add.worst_ms, 12_000);
    }

    #[test]
    fn a_verb_that_only_ever_refused_still_reports() {
        let runs = vec![refused("add", 2), refused("add", 6)];
        let add = &tally(&runs)[0];
        assert_eq!((add.runs, add.refused, add.failed), (2, 2, 0));
        assert_eq!(
            add.median_ms, 0,
            "nothing succeeded, so there is nothing to time"
        );
    }

    #[test]
    fn an_older_clients_rows_stay_faults_and_are_not_reattributed() {
        // ⚠ Rows written before 2026-08-29 said `error` for both. Nothing
        // recorded which they were, so guessing would invent the very split this
        // exists to measure — `failed` falls as that window ages out, and that
        // is not an improvement.
        let runs = vec![
            ran("add", 3, Ended::Error),
            ran("add", 12_000, Ended::Error),
        ];
        let add = &tally(&runs)[0];
        assert_eq!(add.failed, 2);
        assert_eq!(add.refused, 0, "a 3 ms failure was guessed to be a refusal");
    }
}

/// Reading the outcome off the error, and never off its wording.
///
/// ⚠ **This lives in the library so that `tests/` can reach it.** The same
/// classification for the two model checks sat inside `src/bin/task.rs` until
/// 2026-08-26, where nothing could exercise it, and a `minted()` that produced a
/// 32-character "ULID" had every push refused for a day while the whole suite
/// stayed green. A private function in a binary is a function with no seam.
mod classifying {
    use tasks::tasks::commands::{Ended, declined, ended};

    #[test]
    fn a_decline_is_told_apart_from_a_fault() {
        let refused: anyhow::Result<()> = Err(declined("a filing needs a subject"));
        let broke: anyhow::Result<()> = Err(anyhow::anyhow!("reaching the tasks service"));
        assert_eq!(ended(&refused), Ended::Refused);
        assert_eq!(ended(&broke), Ended::Error);
        assert_eq!(ended(&Ok(())), Ended::Ok);
    }

    #[test]
    fn the_caller_still_reads_exactly_what_it_read_before() {
        // The marker rides UNDER the message, where only the classifier looks.
        // If it displaced the words, every refusal in the CLI would start saying
        // "the tool declined" instead of what was actually wrong.
        let why = declined("the subject is the first argument, not a flag");
        assert_eq!(
            format!("{why}"),
            "the subject is the first argument, not a flag"
        );
    }

    #[test]
    fn it_survives_being_wrapped_on_the_way_up() {
        // A refusal deep in a call chain picks up context as it returns. The
        // classification has to see through that, or it silently degrades to
        // "error" for exactly the paths that wrap most.
        use anyhow::Context;
        let deep: anyhow::Result<()> = Err(declined("a check refused this subject"))
            .context("filing a task")
            .context("running `add`");
        assert_eq!(ended(&deep), Ended::Refused);
        assert_eq!(format!("{}", deep.unwrap_err()), "running `add`");
    }

    #[test]
    fn wording_is_not_what_decides_it() {
        // ⚠ The whole reason this is a type. A plain error whose text reads like
        // a refusal must NOT be classified as one — otherwise rewording a line,
        // or a formatter rewriting it, reclassifies a month of runs.
        let sounds_like_one: anyhow::Result<()> =
            Err(anyhow::anyhow!("the tool declined: nothing was filed"));
        assert_eq!(ended(&sounds_like_one), Ended::Error);
    }
}
