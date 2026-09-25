//! What the CLI recorded about itself.
//!
//! ⚠ **Every row this models is a command somebody actually ran** — Pippijn's
//! rule: measure what is actually going on, and do not poll. See
//! `tasks::commands`.

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

/// ⚠ **The error path is usually the FAST one**, so folding it into the
/// percentiles would report the tool fastest on the day it refuses everything.
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

/// ⚠ **Busiest first, because the run count is the weight.**
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

/// Two verbs run equally often keep a stable order between readings.
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

/// ⚠ **Five days is the requirement** — Pippijn: five days of nothing is a
/// problem, anything short of it is not. fleetwatch grades `Silent` beyond 3×
/// the declared interval, so 3× must land exactly on five days, and a quiet
/// weekend must stay inside `Fresh` (1.5×).
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
/// ⚠ **Over both populations, `edit p90` reports the CHECK RATE**: slow edits
/// and density reads match one for one, and the service's share of a checked
/// edit is the same flat cost as an unchecked one.
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
        // ⚠ Rows that do not say know nothing. Folding them into the unchecked
        // population would file slow edits as fast ones.
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
        // A refusal returns without a round trip, so folding the error path in
        // makes the tool look fastest when it refuses most.
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

/// A refusal is the tool working, not breakage: most of `add`'s bad endings
/// are the CLI declining a malformed invocation or the duplicate check refusing.
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
        // Refusals stay out of the percentiles too, for the same reason.
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
        // ⚠ An older client said `error` for both, and guessing would invent
        // the very split this exists to measure.
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
/// ⚠ **In the library so `tests/` can reach it**: a private function in a
/// binary has no seam.
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

/// What a caller reads when a command ends badly.
///
/// ⚠ **The chain is noise on a refusal, and it is what a `tail -3` keeps** —
/// see `commands::said`.
mod what_a_bad_ending_prints {
    use tasks::tasks::commands::{declined, said};

    #[test]
    fn a_refusal_prints_its_sentence_and_nothing_under_it() {
        let text = said(&declined(
            "NOT FILED — #1206 is already open with this exact subject.",
        ));
        assert_eq!(text.lines().count(), 1, "{text}");
        assert!(text.contains("NOT FILED"), "{text}");
        assert!(!text.contains("Caused by"), "the chain survived: {text}");
        assert!(
            !text.contains("the tool declined"),
            "the classifier's marker reached the caller: {text}"
        );
    }

    /// ⚠ **The commonest refusal must carry the marker**: the duplicate check's,
    /// counted as a failed command, would corrupt the measurement from its
    /// busiest source.
    #[test]
    fn a_refused_filing_is_never_counted_as_a_fault() {
        use tasks::tasks::commands::{Ended, ended};
        let refused: anyhow::Result<()> = Err(declined("NOT FILED — a model reading the titles"));
        assert_eq!(ended(&refused), Ended::Refused);
        let broke: anyhow::Result<()> = Err(anyhow::anyhow!("the socket went away"));
        assert_eq!(ended(&broke), Ended::Error);
    }

    #[test]
    fn something_that_actually_went_wrong_keeps_its_chain() {
        // ⚠ The opposite case, and it is why this is a branch rather than a
        // blanket `{}`. A transport failure is diagnosed from its causes, and
        // dropping them to tidy the refusal would cost the one output where the
        // chain is the whole value.
        let text = said(&anyhow::anyhow!("the socket went away").context("reading the open list"));
        assert!(text.contains("reading the open list"), "{text}");
        assert!(
            text.contains("the socket went away"),
            "the cause was dropped: {text}"
        );
    }
}
