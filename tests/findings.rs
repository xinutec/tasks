//! Falsifying a review finding, with the two ways the instrument itself failed.
//!
//! On 2026-09-06 fifty agents read all 712 closed bodies and returned 235
//! findings; 81 did not survive this check. The single requirement that did the
//! work was making a supersession claim quote the LATER text: it rejected **51
//! of 56** stale-layer claims, 28 of them because the "resolving" text sits
//! BEFORE the text it supposedly supersedes.
//!
//! ⚠ **The tests here are the instrument's own mistakes, not hypotheticals.**
//! The first version of this check searched only the body for a `subject-stale`
//! quote and rejected 130 correct findings, because a subject is not in the
//! body. The second tested repetition with `count(quote) > 1` and called all 14
//! repetition findings false, because an agent quotes a lead-in that appears
//! once wrapped around a passage that appears twice. Both were caught by
//! reading, not by the check — which is the argument for pinning them.

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

/// ⚠ **The check that did the work.** 28 of 56 stale-layer claims put the
/// resolving text BEFORE the problem text, which is not a supersession — it is
/// the reading order reversed, and it is what a model produces when it has
/// pattern-matched rather than read.
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

/// ⚠ **Instrument failure 1**, which rejected 130 correct findings: the subject
/// is not part of the body, so a `subject-stale` quote must be looked for in
/// both.
#[test]
fn a_subject_quote_is_looked_for_in_the_subject() {
    let subject = "the guard is missing on every write path";
    let v = falsify(&f(Kind::SubjectStale, subject, None), subject, BODY);
    assert!(
        v.supported(),
        "a subject quote must not be hunted in the body alone: {v:?}"
    );
}

/// ⚠ **Instrument failure 2**: counting occurrences of the whole quote said all
/// 14 repetition findings were false. The agent quotes one instance plus a
/// lead-in that appears once. So a repetition finding must name BOTH.
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
