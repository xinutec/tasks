//! Reading what a model said about a filing.
//!
//! ⚠ **The strings here are real answers, not invented ones.** Every `said` in
//! this file was produced by `claude-haiku-4-5` against the live list on
//! 2026-08-13, replaying five tasks whose truth was known — which is also where
//! the wrappers come from. A model asked for `#<id> -- <clause>` and nothing
//! else supplies a bullet, a bold marker or a preamble often enough that
//! tolerating them is the feature rather than a nicety.
//!
//! The half these cannot reach is whether the *answer* is right, which is a
//! property of the prompt and of the model behind it. That was measured by
//! replay rather than asserted here: 5/5 on open tasks, 3/5 once done and
//! dropped rows were included. See the module's own documentation — this file
//! pins that whatever comes back is read correctly.

use tasks::tasks::duplicates::{Match, parse, prompt, report};

/// The id the replayed filings carried, so a test says which task is asking.
const FILED: u64 = 812;

#[test]
fn a_clean_list_says_nothing() {
    assert_eq!(parse("NONE", FILED), Vec::new());
}

#[test]
fn the_ordinary_answer_is_an_id_and_a_clause() {
    let said = "#255 -- both describe something suspended that has never run, \
                blocking on a decision to un-suspend";
    assert_eq!(
        parse(said, FILED),
        vec![Match {
            id: 255,
            why: "both describe something suspended that has never run, blocking on a \
                  decision to un-suspend"
                .into(),
        }]
    );
}

#[test]
fn several_matches_keep_their_order() {
    // Verbatim, including the two-line shape: this is what the 686 closed rows
    // bought on #812, and both of them were wrong.
    let said = "#671 -- both describe failures in the picade health component\n\
                #70 -- related picade systems offline issues with fleetwatch not \
                handling them correctly";
    let found = parse(said, FILED);
    assert_eq!(
        found.iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![671, 70]
    );
}

#[test]
fn a_model_that_explains_itself_is_not_quoted() {
    // ⚠ The whole reason lines are dropped rather than passed through: this is
    // printed into a conversation that has just filed a task, and a paragraph
    // about the model's reasoning is bytes every session pays for.
    let said = "Looking at the list, I found one likely match:\n\n\
                #726 -- geb is configured as backup target; intermittent setting update\n\n\
                Let me know if you would like me to look more closely.";
    assert_eq!(
        parse(said, FILED),
        vec![Match {
            id: 726,
            why: "geb is configured as backup target; intermittent setting update".into(),
        }]
    );
}

#[test]
fn bullets_and_bold_are_wrappers_rather_than_answers() {
    for said in [
        "- #255 -- the same suspended cron",
        "* **#255** — the same suspended cron",
        "1. `#255`: the same suspended cron",
    ] {
        assert_eq!(
            parse(said, FILED),
            vec![Match {
                id: 255,
                why: "the same suspended cron".into()
            }],
            "{said}"
        );
    }
}

#[test]
fn a_task_is_never_its_own_duplicate() {
    // The corpus excludes it, so this is a model echoing the number it was
    // asked about. Reported, it would read as a defect in the service rather
    // than as a bad guess.
    let said = format!("#{FILED} -- this is the same task");
    assert_eq!(parse(&said, FILED), Vec::new());
}

#[test]
fn one_task_is_named_once() {
    let said = "#255 -- the suspended cron\n#255 -- and it has never run";
    assert_eq!(parse(said, FILED).len(), 1);
}

#[test]
fn an_id_with_nothing_to_say_is_not_a_finding() {
    // ⚠ **A bare number costs a `task show` to learn it was not worth one.**
    // The clause is what makes a match cheap to dismiss, so a line without one
    // is not an answer to the question that was asked.
    assert_eq!(parse("#255", FILED), Vec::new());
    assert_eq!(parse("#255 --", FILED), Vec::new());
}

#[test]
fn a_sentence_with_no_id_is_not_a_match() {
    assert_eq!(
        parse("I could not find any duplicates in the list.", FILED),
        Vec::new()
    );
}

#[test]
fn at_most_three_are_carried() {
    let said = "#1 -- one\n#2 -- two\n#3 -- three\n#4 -- four\n#5 -- five";
    assert_eq!(parse(said, FILED).len(), 3);
}

#[test]
fn the_prompt_carries_the_filing_and_the_list() {
    let corpus = [
        (255, "the cron has simply never run".to_string()),
        (726, "geb holds the backups".to_string()),
    ];
    let text = prompt("a new thing entirely", &corpus);
    assert!(
        text.contains("a new thing entirely"),
        "the subject asked about"
    );
    assert!(
        text.contains("255 | the cron has simply never run"),
        "the list"
    );
    assert!(
        text.contains("726 | geb holds the backups"),
        "all of the list"
    );
}

#[test]
fn the_prompt_says_what_is_not_a_duplicate() {
    // ⚠ **This is the sentence the measurement turned on.** Without the
    // negative half, an all-pairs sweep returned nine groups of which one was
    // real — same-area, same-repo and same-technology are what a model reaches
    // for when nobody tells it not to.
    let text = prompt("anything", &[(1, "something".to_string())]);
    assert!(text.contains("NOT the same problem"));
    assert!(text.contains("NOT duplicates"));
}

#[test]
fn the_report_says_it_is_a_guess() {
    // ⚠ Not decoration. Every other line this CLI prints is read off the
    // service; this one is inference from titles alone, and the difference has
    // to survive being pasted somewhere without its context.
    let text = report(&[Match {
        id: 255,
        why: "the same suspended cron".into(),
    }]);
    assert!(text.contains("a guess"), "{text}");
    assert!(text.contains("#255"), "{text}");
    assert!(text.contains("the same suspended cron"), "{text}");
}
