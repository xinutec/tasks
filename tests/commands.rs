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
            median_ms: 0,
            p90_ms: 0,
            worst_ms: 0,
        }]
    );
}
