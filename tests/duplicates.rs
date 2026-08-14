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

use tasks::tasks::duplicates::{Match, collision, parse, prompt, refusal, same_subject};

/// An id that is deliberately NOT in [`corpus`], so a test can assert that a
/// number the model was never shown cannot come back as a match.
const NOT_ON_THE_LIST: u64 = 812;

/// The open list the replayed answers were given, reduced to what `parse` uses.
///
/// ⚠ **Every id a test expects to survive has to be in here.** `parse` discards
/// an id that is not on the list it was given, which is what stops an invented
/// number refusing somebody's filing — so a corpus missing an id makes a test
/// fail for a reason that has nothing to do with what it is checking.
fn corpus() -> Vec<(u64, String)> {
    [1, 2, 3, 4, 5, 70, 255, 671, 726]
        .into_iter()
        .map(|id| (id, format!("open task {id}")))
        .collect()
}

#[test]
fn a_clean_list_says_nothing() {
    assert_eq!(parse("NONE", &corpus()), Vec::new());
}

#[test]
fn the_ordinary_answer_is_an_id_and_a_clause() {
    let said = "#255 -- both describe something suspended that has never run, \
                blocking on a decision to un-suspend";
    assert_eq!(
        parse(said, &corpus()),
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
    let found = parse(said, &corpus());
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
        parse(said, &corpus()),
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
            parse(said, &corpus()),
            vec![Match {
                id: 255,
                why: "the same suspended cron".into()
            }],
            "{said}"
        );
    }
}

#[test]
fn an_id_that_was_never_on_the_list_is_not_a_match() {
    // ⚠ **Load-bearing since the answer started refusing filings.** A number the
    // model invented — or echoed from the prose it was given — must not be able
    // to block work. The corpus is the only thing that says which ids were real.
    let said = format!("#{NOT_ON_THE_LIST} -- this is the same task");
    assert_eq!(parse(&said, &corpus()), Vec::new());
}

#[test]
fn one_task_is_named_once() {
    let said = "#255 -- the suspended cron\n#255 -- and it has never run";
    assert_eq!(parse(said, &corpus()).len(), 1);
}

#[test]
fn an_id_with_nothing_to_say_is_not_a_finding() {
    // ⚠ **A bare number costs a `task show` to learn it was not worth one.**
    // The clause is what makes a match cheap to dismiss, so a line without one
    // is not an answer to the question that was asked.
    assert_eq!(parse("#255", &corpus()), Vec::new());
    assert_eq!(parse("#255 --", &corpus()), Vec::new());
}

#[test]
fn a_sentence_with_no_id_is_not_a_match() {
    assert_eq!(
        parse("I could not find any duplicates in the list.", &corpus()),
        Vec::new()
    );
}

#[test]
fn at_most_three_are_carried() {
    let said = "#1 -- one\n#2 -- two\n#3 -- three\n#4 -- four\n#5 -- five";
    assert_eq!(parse(said, &corpus()).len(), 3);
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
fn a_refusal_admits_it_is_a_model_reading_titles() {
    // ⚠ Not decoration. Every other refusal this CLI prints is a rule — a
    // missing priority, a subject that is really a body. This one is a small
    // model's opinion, and a caller told "this is a duplicate" checks nothing
    // where a caller told what was matched opens the task.
    let text = refusal(&[Match {
        id: 255,
        why: "the same suspended cron".into(),
    }]);
    assert!(text.contains("model"), "{text}");
    assert!(text.contains("#255"), "{text}");
    assert!(text.contains("the same suspended cron"), "{text}");
}

#[test]
fn a_refusal_says_nothing_was_filed_and_how_to_file_it_anyway() {
    // ⚠ **Both halves, or the refusal is worse than the duplicate.** A caller
    // that cannot tell whether the task landed re-runs and makes a real one —
    // which is exactly how #859 and #860 happened, 46 seconds apart. And a
    // refusal with no way past it turns a false positive into lost work, when
    // the body is still sitting in the command the caller just ran.
    let text = refusal(&[Match {
        id: 689,
        why: "the same signal.dhall apply".into(),
    }]);
    assert!(text.contains("nothing was filed"), "{text}");
    assert!(text.contains("--no-duplicate-check"), "{text}");
}

#[test]
fn the_same_subject_twice_is_found_without_a_model() {
    // The pair that made this exist: identical subjects, 46 seconds apart,
    // caught only by a Haiku call that ran after the second one was already on
    // the list.
    let subject = "health is public and carries your home location to ~100 m";
    let corpus = [
        (
            853,
            "DONE: three place names renamed to synthetics".to_string(),
        ),
        (859, subject.to_string()),
    ];
    assert_eq!(same_subject(subject, &corpus), Some(859));
    assert!(collision(859).contains("task edit 859"));
}

#[test]
fn case_and_surrounding_space_do_not_make_a_second_task() {
    let corpus = [(859, "  MEMORY.md is 21.7KB  ".to_string())];
    assert_eq!(same_subject("memory.md IS 21.7KB", &corpus), Some(859));
}

#[test]
fn a_subject_that_merely_starts_the_same_is_not_a_collision() {
    // ⚠ **Only equality refuses.** Anything looser is the model's question, and
    // the module's measurement is why it may not block a filing: two tasks that
    // open with the same words are the ordinary case, not a mistake.
    let corpus = [(859, "MEMORY.md is 21.7KB".to_string())];
    assert_eq!(
        same_subject("MEMORY.md is 21.7KB and still growing", &corpus),
        None
    );
}
