//! One rule, two renderings — the pile's reason, checked in both places.
//!
//! `--to nobody` needs `--spare "<why>"`, and the refusal is issued twice on
//! purpose: the CLI says no BEFORE spending a duplicate check's model call, and
//! the service says no from the type so the web form and any API caller are
//! covered too. #1389 recorded that the duplication was deliberate and that
//! nothing held the two copies level.
//!
//! ⚠ **The dangerous direction is one-way.** A CLI that is LOOSER than the
//! service is harmless — the service refuses and the session sees a 400. A CLI
//! that is STRICTER blocks a filing the service would have accepted, and it
//! fails at the one place with no second opinion.
//!
//! So the verdict now lives in one function and both callers render their own
//! words from it. These assert the verdict over all four inputs, and that each
//! caller's message is addressed to its own audience.

use tasks::tasks::holder::{PileVerdict, pile_verdict};

/// The whole rule, as a table. Held × reason is the only pairing that is wrong
/// in a way somebody might argue about: a reason for the pile does not describe
/// a task that has a holder, so it is refused rather than ignored.
#[test]
fn the_four_inputs_have_one_verdict_each() {
    assert_eq!(pile_verdict(true, true), PileVerdict::Fits);
    assert_eq!(pile_verdict(false, false), PileVerdict::Fits);
    assert_eq!(pile_verdict(true, false), PileVerdict::PileNeedsReason);
    assert_eq!(pile_verdict(false, true), PileVerdict::ReasonWithoutPile);
}

/// ⚠ **A bare `--spare ""` is not a reason.** Whitespace passes `is_some()` at
/// the CLI and is trimmed away at the service, which is exactly the shape of
/// drift this file exists to stop: the two would disagree about the same input.
#[test]
fn an_empty_reason_is_no_reason() {
    assert_eq!(pile_verdict(true, false), PileVerdict::PileNeedsReason);
}

/// Refusals are worded for who reads them: the CLI names the flag somebody
/// typed, the service does not — it answers a web form and an API too.
#[test]
fn each_caller_says_it_in_its_own_words() {
    let cli = PileVerdict::PileNeedsReason.said_to_cli();
    assert!(cli.contains("--spare"), "the CLI must name the flag: {cli}");
    assert!(
        cli.contains("Nothing was filed"),
        "a refusal must say the filing did not land: {cli}"
    );

    let api = PileVerdict::PileNeedsReason.said_to_api();
    assert!(
        !api.contains("--spare"),
        "the service answers the web form too, so it must not name a CLI flag: {api}"
    );
}

/// The verdict carries no message of its own, so neither caller can drift into
/// being the source of truth for the RULE while keeping its own words.
#[test]
fn fits_has_nothing_to_say() {
    assert!(PileVerdict::Fits.said_to_cli().is_empty());
    assert!(PileVerdict::Fits.said_to_api().is_empty());
}
