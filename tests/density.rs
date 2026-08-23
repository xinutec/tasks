//! What a model is asked about a body, and what is done with the answer.
//!
//! ⚠ **The half under test is the half that is NOT a guess.** Whether a model's
//! reading of a body is right cannot be asserted here, and these do not try:
//! what they pin is that the standard reaches it, that a body which is fine
//! costs nothing, and that an answer nobody can read is silence rather than
//! noise. The advice never refuses a write, so every failure mode below is a
//! session's attention rather than a session's work.

use tasks::tasks::density::{self, RUBRIC, SAMPLER};

#[test]
fn a_body_nobody_has_added_much_to_is_not_worth_a_model() {
    assert!(!density::worth_asking(0));
    assert!(!density::worth_asking(SAMPLER));
    assert!(density::worth_asking(SAMPLER + 1));
}

#[test]
fn the_standard_reaches_the_model_that_grades_against_it() {
    let asked = density::prompt(982, 40_000, "a body");
    // Not "mentions a rubric" — the exact text `task edit --help` prints, so
    // that a session written to one standard is marked against that one.
    assert!(
        asked.contains(RUBRIC),
        "the rubric itself must be in the prompt"
    );
    assert!(
        asked.contains("#982"),
        "the id, so the answer can name the task"
    );
    assert!(
        asked.contains("40000"),
        "how much has accreted since a rewrite"
    );
    assert!(asked.contains("a body"), "the body it is judging");
}

#[test]
fn the_rubric_asks_for_density_and_never_for_fewer_words() {
    // ⚠ Told to compress, a model drops the numbers and keeps the prose,
    // because prose reads like the argument — and a body is believed for its
    // measurements. The one-sided second rule is what stops that.
    assert!(RUBRIC.contains("No claim without its measurement"));
    assert!(RUBRIC.contains("SUPERSESSION"));
    assert!(
        !RUBRIC.to_lowercase().contains("as few words"),
        "a word budget is cut from the evidence first"
    );
}

#[test]
fn a_body_that_holds_together_costs_nothing_to_say_so() {
    assert_eq!(density::advice("DENSE"), None);
    assert_eq!(density::advice("  dense\n"), None);
    // Said the useful part first and then explained itself anyway.
    assert_eq!(density::advice("DENSE\nIt reads top down."), None);
}

#[test]
fn an_answer_nobody_can_read_is_silence() {
    assert_eq!(density::advice(""), None);
    assert_eq!(density::advice("   \n  \n"), None);
}

#[test]
fn what_is_wrong_is_printed_as_a_guess_with_a_way_out() {
    let said = "The conclusion is at 82% of the body.\nThe 08-12 table is marked stale above it.";
    let advice = density::advice(said).expect("something to say");
    assert!(advice.contains("it is a guess"), "marked as inference");
    assert!(advice.contains("82%"), "what the model actually said");
    assert!(advice.contains("--body -"), "the command that fixes it");
}

#[test]
fn a_model_that_will_not_stop_talking_is_cut_off() {
    let said = (1..=9).map(|n| format!("line {n}\n")).collect::<String>();
    let advice = density::advice(&said).expect("something to say");
    assert!(advice.contains("line 4"));
    // Four lines was the bound put to the model. A checker that answers a body
    // being too long with a page of its own has joined the problem.
    assert!(!advice.contains("line 5"), "at most four lines survive");
}
