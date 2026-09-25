//! Falsifying a review finding, with the two ways the instrument itself failed.
//!
//! The requirement that does the work is making a supersession claim quote the
//! LATER text, which rejects most stale-layer claims a review makes — many
//! because the "resolving" text sits BEFORE the text it supersedes.
//!
//! ⚠ **Two tests pin the instrument's own mistakes**: searching only the body
//! rejects every correct `subject-stale` finding, and counting occurrences of
//! a whole quote calls every repetition false. Both were caught by reading, not
//! by the check.

use tasks::tasks::findings::{Finding, Kind, Reason, falsify};

fn f(kind: Kind, problem: &str, resolving: Option<&str>) -> Finding {
    Finding {
        kind,
        quote_problem: problem.to_string(),
        quote_resolving: resolving.map(str::to_string),
        quote_second: None,
    }
}

const BODY: &str = "\
The guard was missing and every write path was open.

## Fixed 2026-08-20

The guard now covers every write path.
";

#[test]
fn a_quote_that_is_not_in_the_task_is_refused() {
    let v = falsify(
        &f(Kind::Verbose, "a sentence nobody wrote", None),
        "subj",
        BODY,
    );
    assert!(v.rejected_for(Reason::ProblemNotVerbatim), "{v:?}");
}

#[test]
fn a_supersession_needs_the_later_text_quoted() {
    let v = falsify(
        &f(Kind::StaleLayer, "The guard was missing", None),
        "subj",
        BODY,
    );
    assert!(v.rejected_for(Reason::NoResolvingQuote), "{v:?}");
}

/// ⚠ **The check that does the work.** Resolving text BEFORE the problem text
/// is not a supersession but the reading order reversed — what a model
/// produces when it has pattern-matched rather than read.
#[test]
fn resolving_text_that_comes_first_is_refused() {
    let v = falsify(
        &f(
            Kind::StaleLayer,
            "The guard now covers every write path.",
            Some("The guard was missing"),
        ),
        "subj",
        BODY,
    );
    assert!(v.rejected_for(Reason::ResolvingComesFirst), "{v:?}");
}

#[test]
fn a_real_supersession_survives() {
    let v = falsify(
        &f(
            Kind::StaleLayer,
            "The guard was missing",
            Some("The guard now covers every write path."),
        ),
        "subj",
        BODY,
    );
    assert!(v.supported(), "{v:?}");
}

/// ⚠ **Instrument mistake**: the subject is not part of the body, so a
/// `subject-stale` quote must be looked for in both.
#[test]
fn a_subject_quote_is_looked_for_in_the_subject() {
    let subject = "the guard is missing on every write path";
    let v = falsify(&f(Kind::SubjectStale, subject, None), subject, BODY);
    assert!(
        v.supported(),
        "a subject quote must not be hunted in the body alone: {v:?}"
    );
}

/// ⚠ **Instrument mistake**: counting occurrences of the whole quote calls
/// every repetition false, because a model quotes one instance plus a lead-in
/// that appears once. So a repetition finding must name BOTH.
#[test]
fn repetition_must_name_both_occurrences() {
    let twice = "Run it twice.\nsome other line\nRun it twice.\n";
    let mut one = f(Kind::Repetition, "Run it twice.", None);
    let v = falsify(&one, "subj", twice);
    assert!(v.rejected_for(Reason::RepetitionNeedsBoth), "{v:?}");

    one.quote_second = Some("Run it twice.".to_string());
    let v = falsify(&one, "subj", twice);
    assert!(v.supported(), "two real occurrences: {v:?}");
}

/// And a claimed second occurrence that is really the same one is refused —
/// otherwise quoting the same span twice passes trivially.
#[test]
fn the_second_occurrence_must_be_a_different_place() {
    let once = "Only said once.\n";
    let g = Finding {
        kind: Kind::Repetition,
        quote_problem: "Only said once.".into(),
        quote_resolving: None,
        quote_second: Some("Only said once.".into()),
    };
    let v = falsify(&g, "subj", once);
    assert!(v.rejected_for(Reason::RepetitionNeedsBoth), "{v:?}");
}
