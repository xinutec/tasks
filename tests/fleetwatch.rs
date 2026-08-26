//! What fleetwatch's ingest will and will not accept from us.
//!
//! ⚠ **These are the tests that were missing, and their absence is the whole
//! story.** `fleetwatch::minted` returned 32 hex characters and its own comment
//! called the result "ULID-shaped". A ULID is 26 characters of Crockford
//! base32, and `ingest` parses the id before storing anything, answering 422 on
//! failure — so the task-timings series was empty from the day it shipped while
//! every test in this repo stayed green. None of them could see this module: it
//! was private, inside `src/bin/task.rs`, where `tests/` cannot reach.
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
