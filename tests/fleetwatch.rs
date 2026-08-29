//! What fleetwatch's ingest will and will not accept from us.
//!
//! ⚠ **These are the tests that were missing, and their absence is the whole
//! story.** `fleetwatch::minted` returned 32 hex characters and its own comment
//! called the result "ULID-shaped". A ULID is 26 characters of Crockford
//! base32, and `ingest` parses the id before storing anything, answering 422 on
//! failure. The series holds exactly ONE report — 2026-08-25T18:00:55Z, from a
//! development build whose id happened to be 26 characters — and nothing after
//! it, because `minted()` was widened to 32 before the commit. Every test in
//! this repo stayed green throughout. None of them could see this module: it was
//! private, inside `src/bin/task.rs`, where `tests/` cannot reach.
//!
//! So each test here asserts one rule the RECEIVER enforces, not one this crate
//! happens to implement.

use tasks::tasks::fleetwatch;

/// `ingest` calls `Ulid::from_string` on the id and rejects the report before
/// storing it. Parsing with the same crate the server uses cannot drift from
/// what the server accepts; asserting a length alone could.
#[test]
fn a_minted_id_is_a_ulid_the_server_will_accept() {
    let id = fleetwatch::minted();
    assert_eq!(id.len(), 26, "a ULID is 26 characters, got {id:?}");
    assert!(
        ulid::Ulid::from_string(&id).is_ok(),
        "{id:?} does not parse as a ULID"
    );
}

/// Two reports minted in one run must differ.
///
/// The id is the idempotency key. A content-derived one would make a second
/// report carrying identical tallies read as a replay of the first and be
/// dropped — which shows on the chart as a gap exactly when the tracker was
/// quiet but alive, the one state the series exists to distinguish.
#[test]
fn two_ids_minted_together_differ() {
    assert_ne!(fleetwatch::minted(), fleetwatch::minted());
}

/// The other two shapes `ingest` refuses: an empty label, and an identity field
/// over 255 bytes — which the DB would TRUNCATE, silently splitting one trend
/// into two. Both are built from strings the service supplies, so neither is
/// under this crate's control at the point of use.
#[test]
fn every_check_carries_an_addressable_identity() {
    let report = serde_json::json!({
        "interval_s": 3600,
        "commands": [{"verb": "add", "runs": 4, "p90_ms": 21000, "failed": 1}],
        "checks": [{"kind": "filing", "runs": 4, "timeout": 1, "p90_ms": 19000}],
    });
    let built = fleetwatch::checks(&report);
    assert!(!built.is_empty(), "the fixture should produce checks");
    for c in &built {
        let label = c["label"].as_str().expect("a label");
        let section = c["section"].as_str().expect("a section");
        assert!(!label.trim().is_empty(), "empty label in {c}");
        assert!(label.len() <= 255, "label over 255 bytes: {label:?}");
        assert!(section.len() <= 255, "section over 255 bytes: {section:?}");
    }
}

/// What the tally already knew and the push threw away.
///
/// ⚠ **Three numbers reached the service and stopped there.** `checks::Tally`
/// has carried `spoke`, `quiet`, `timeout` and `error` per kind since 0010, and
/// `commands::Tally` has carried `failed` per verb — and `checks()` turned
/// exactly one of them into a line, for one kind. Measured by hand on
/// 2026-08-29: 229 of 268 density reads spoke and 37 never answered, both
/// invisible on every chart. The gap was not in the instrument; it was in the
/// last ten lines before the wire.
mod what_reaches_the_wire {
    use super::*;

    fn built() -> Vec<serde_json::Value> {
        fleetwatch::checks(&serde_json::json!({
            "interval_s": 3600,
            "commands": [
                {"verb": "add", "runs": 10, "p90_ms": 21000, "failed": 3},
                {"verb": "list", "runs": 40, "p90_ms": 200, "failed": 0},
            ],
            "checks": [
                {"kind": "filing", "runs": 208, "quiet": 109, "spoke": 97,
                 "timeout": 0, "error": 2, "p90_ms": 18000},
                {"kind": "density", "runs": 268, "quiet": 2, "spoke": 229,
                 "timeout": 37, "error": 0, "p90_ms": 90000},
            ],
        }))
    }

    fn value_of(label: &str) -> f64 {
        built()
            .into_iter()
            .find(|c| c["label"] == label)
            .unwrap_or_else(|| panic!("no line labelled {label:?}"))["value"]
            .as_f64()
            .expect("a numeric value")
    }

    #[test]
    fn a_density_read_that_never_answered_is_on_a_line_of_its_own() {
        // The 14% that was charted nowhere. `filing` had this line and
        // `density` — the kind that runs most — did not.
        assert_eq!(value_of("density checks that never answered"), 37.0);
    }

    #[test]
    fn how_often_a_check_spoke_is_a_value_and_not_a_sentence() {
        assert_eq!(value_of("density checks that spoke"), 229.0);
        assert_eq!(value_of("filing checks that spoke"), 97.0);
    }

    #[test]
    fn a_failing_command_is_countable() {
        // It was inside the observed text of a LATENCY line, where nothing can
        // chart it or band it. Aggregated, because a series per verb to carry a
        // number that is nearly always zero crowds out the ones that move.
        assert_eq!(value_of("commands that failed"), 3.0);
        let observed = built()
            .into_iter()
            .find(|c| c["label"] == "commands that failed")
            .expect("the line")["observed"]
            .as_str()
            .expect("a sentence")
            .to_string();
        assert!(
            observed.contains("`add`"),
            "which verb is the finding: {observed}"
        );
    }

    #[test]
    fn an_error_joins_a_timeout_and_quiet_never_does() {
        // Both mean the input was never judged. `quiet` is the opposite finding
        // — it ran and had nothing to say — and summing it in would report a
        // well-behaved tool exactly when the tool had stopped running.
        assert_eq!(
            value_of("filing checks that never answered"),
            2.0,
            "0 timeouts and 2 errors is 2 unanswered"
        );
        assert_eq!(value_of("filing checks that spoke"), 97.0);
        assert_ne!(
            value_of("density checks that never answered"),
            39.0,
            "the 2 quiet reads were counted as failures to answer"
        );
    }

    #[test]
    fn only_the_filing_line_claims_a_bound() {
        // Zero unchecked filings is defensible: that is how a duplicate gets in.
        // A density read is advisory with a measured 14% baseline and no derived
        // bound, so a verdict there would publish a guess as a finding.
        let verdict = |label: &str| {
            built()
                .into_iter()
                .find(|c| c["label"] == label)
                .expect("the line")["verdict"]
                .as_str()
                .expect("a verdict")
                .to_string()
        };
        assert_eq!(verdict("density checks that never answered"), "pass");
        assert_eq!(verdict("filing checks that never answered"), "warn");
    }
}

/// The fixtures above are hand-written JSON, and that is a gap of its own.
///
/// ⚠ **A field renamed on `Tally` would zero every line here and fail nothing.**
/// `checks()` reads `line["spoke"]` out of a `Value` and falls back to 0, so the
/// only thing tying its keys to the service's wire format is that somebody typed
/// them the same way twice. This drives the REAL structs through `serde` and
/// asserts the numbers survive — the same shape as the ULID bug this file opens
/// with: the local side was self-consistent and disagreed with the other end.
#[test]
fn the_keys_are_the_ones_the_service_actually_sends() {
    use tasks::tasks::checks::{Kind, Tally as CheckTally};
    use tasks::tasks::commands::Tally as CommandTally;

    let report = serde_json::json!({
        "interval_s": 3600,
        "commands": [serde_json::to_value(CommandTally {
            verb: "edit".into(),
            runs: 12,
            failed: 4,
            median_ms: 900,
            p90_ms: 27_000,
            worst_ms: 31_000,
        })
        .expect("a command tally serialises")],
        "checks": [serde_json::to_value(CheckTally {
            kind: Kind::Density,
            runs: 268,
            quiet: 2,
            spoke: 229,
            timeout: 37,
            error: 0,
            median_ms: 36_000,
            p90_ms: 90_000,
            worst_ms: 150_000,
        })
        .expect("a check tally serialises")],
    });

    let built = fleetwatch::checks(&report);
    let value = |label: &str| {
        built
            .iter()
            .find(|c| c["label"] == label)
            .unwrap_or_else(|| panic!("no line labelled {label:?}"))["value"]
            .as_f64()
            .expect("a numeric value")
    };
    assert_eq!(value("density checks that spoke"), 229.0);
    assert_eq!(value("density checks that never answered"), 37.0);
    assert_eq!(value("commands that failed"), 4.0);
    assert_eq!(value("edit latency"), 27_000.0);
}

/// The lines about the WORK, driven from the real tally.
///
/// ⚠ **Same gap, same guard as `the_keys_are_the_ones_the_service_actually_sends`.**
/// `checks()` reads `work["sprawling"]` out of a `Value` and falls back to
/// skipping the line, so a field renamed on `work::Tally` would silently drop
/// the series and fail nothing. This drives the struct through serde.
mod the_work {
    use super::*;
    use tasks::tasks::work::Tally;

    fn from(work: Option<Tally>) -> Vec<serde_json::Value> {
        let mut report = serde_json::json!({
            "interval_s": 3600,
            "commands": [],
            "checks": [],
        });
        if let Some(work) = work {
            report["work"] = serde_json::to_value(work).expect("a work tally serialises");
        }
        fleetwatch::checks(&report)
    }

    fn standing() -> Tally {
        Tally {
            open: 173,
            unheld: 0,
            overdue: 0,
            urgent: 14,
            blocked: 6,
            sprawling: 3,
        }
    }

    fn line(built: &[serde_json::Value], label: &str) -> serde_json::Value {
        built
            .iter()
            .find(|c| c["label"] == label)
            .unwrap_or_else(|| panic!("no line labelled {label:?}"))
            .clone()
    }

    #[test]
    fn the_backlog_reaches_the_wire_with_the_names_the_service_sends() {
        let built = from(Some(standing()));
        assert_eq!(line(&built, "open tasks")["value"], 173.0);
        assert_eq!(line(&built, "tasks in the pile")["value"], 0.0);
        assert_eq!(line(&built, "tasks at P0 or P1")["value"], 14.0);
        assert_eq!(line(&built, "tasks blocked on open work")["value"], 6.0);
        assert_eq!(
            line(&built, "bodies carrying an unaddressed finding")["value"],
            3.0,
            "the number 0014 exists to move is the one that must not go missing"
        );
    }

    #[test]
    fn a_count_that_could_not_be_taken_sends_nothing() {
        // ⚠ **Zero is a legitimate reading** — it is what an empty tracker looks
        // like — so a failed query must not answer with one. `standing()` is
        // called with `.ok()` in the route for exactly this, and the section is
        // skipped rather than zeroed.
        let built = from(None);
        assert!(
            !built.iter().any(|c| c["label"] == "open tasks"),
            "a missing tally published itself as an empty backlog: {built:?}"
        );
    }

    #[test]
    fn a_missed_deadline_is_the_one_thing_here_that_claims_a_bound() {
        // Zero is defensible: a deadline is the only thing in this tracker that
        // somebody outside it set, the digest already shouts OVERDUE, and the
        // rank escalates a week out. The rest have no derived bound, so a
        // verdict on them would publish a guess as a measurement.
        let clear = from(Some(standing()));
        assert_eq!(line(&clear, "tasks past their deadline")["verdict"], "pass");
        assert_eq!(line(&clear, "open tasks")["verdict"], "pass");

        let missed = from(Some(Tally {
            overdue: 2,
            ..standing()
        }));
        assert_eq!(
            line(&missed, "tasks past their deadline")["verdict"],
            "warn"
        );
        assert_eq!(
            line(&missed, "tasks at P0 or P1")["verdict"],
            "pass",
            "urgency is a level somebody chose, not a state that changed"
        );
    }
}
