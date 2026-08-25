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

use tasks::tasks::duplicates::{
    Match, Settled, advice, candidates, collision, edged, parse, prompt, refusal, same_subject,
    settled_block, split, worth_reading,
};

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
    let text = prompt("a new thing entirely", &corpus, false);
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
    let text = prompt("anything", &[(1, "something".to_string())], false);
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

/// A task cannot be a duplicate of the task it has just declared it waits for.
///
/// ⚠ **The edge is the filer's own statement that these are two different
/// pieces of work, in the same command.** 2026-08-17: filing phonos's
/// language-decision task with `--blocked-on 984` was refused for resembling
/// #984. Overruling that costs one re-run — the damage is that a check which
/// refuses for a reason the filer has already answered teaches sessions to
/// reach for `--no-duplicate-check` by reflex, and then it catches nothing.
#[test]
fn what_a_filing_waits_for_is_not_shown_to_the_reader() {
    let corpus = vec![
        (984, "Decide the language phonos is written in".to_string()),
        (985, "Something else entirely".to_string()),
    ];
    let shown = candidates(&corpus, &[984]);
    assert_eq!(shown, vec![(985, "Something else entirely".to_string())]);
}

#[test]
fn a_filing_that_waits_for_nothing_sees_the_whole_list() {
    let corpus = vec![
        (1, "one".to_string()),
        (2, "two".to_string()),
        (3, "three".to_string()),
    ];
    assert_eq!(candidates(&corpus, &[]), corpus);
    // Order is the list's, not the filter's: `parse` reads ids back against
    // this same slice, and a reordered corpus would still be correct but would
    // make a diff of two runs unreadable.
    assert_eq!(
        candidates(&corpus, &[2]),
        vec![(1, "one".to_string()), (3, "three".to_string())]
    );
}

/// The other half of the guard, deliberately left alone.
///
/// ⚠ **An identical title is one task whatever edge was declared.** Filing
/// `--blocked-on 42` a subject character-identical to #42's is a mistake being
/// made twice, not an ordering, so string equality still refuses it — and it
/// runs against the WHOLE list, before this filter narrows anything.
#[test]
fn an_identical_subject_is_still_a_collision_with_what_it_waits_for() {
    let corpus = vec![(42, "Fix the parser".to_string())];
    assert_eq!(same_subject("Fix the parser", &corpus), Some(42));
}

/// A closed task's remedy is not the same sentence as an open one's.
///
/// ⚠ **`reopen` is the whole point.** A session told only that #863 resembles
/// its filing will file anyway; told that the move is `task reopen 863`, it has
/// somewhere to go. This is the line that turns a match into an action.
#[test]
fn a_closed_match_names_reopen_and_says_the_task_still_landed() {
    let text = advice(
        &[(
            Match {
                id: 863,
                why: "both compact MEMORY.md by merging files".into(),
            },
            Settled {
                id: 863,
                subject: "MEMORY.md is 21.7KB".into(),
                dropped: true,
            },
        )],
        961,
        34,
    );
    assert!(text.contains("#863"), "{text}");
    assert!(text.contains("task reopen"), "{text}");
    assert!(
        text.contains("filed"),
        "the caller must know the task landed: {text}"
    );
    assert!(
        text.contains("961") && text.contains("34"),
        "the corpus it read: {text}"
    );
}

/// ⚠ **A dropped task is not reported as a decision.** `task drop` records a
/// status and no reason, so "dropped" alone does not mean anybody decided
/// anything: #863 is dropped, carries a full merge plan, and states no reason.
/// A model asked about it reported that it "concluded the work wasn't
/// justified", which the row does not say — so this line must point at the task
/// rather than assert what its status means.
#[test]
fn a_dropped_match_sends_the_reader_to_the_task_not_to_its_status() {
    let dropped = advice(
        &[(
            Match {
                id: 863,
                why: "same work".into(),
            },
            Settled {
                id: 863,
                subject: "x".into(),
                dropped: true,
            },
        )],
        10,
        0,
    );
    assert!(dropped.contains("reason is in the task"), "{dropped}");
    let done = advice(
        &[(
            Match {
                id: 689,
                why: "same work".into(),
            },
            Settled {
                id: 689,
                subject: "y".into(),
                dropped: false,
            },
        )],
        10,
        0,
    );
    assert!(done.contains("already done"), "{done}");
    assert!(!done.contains("reason is in the task"), "{done}");
}

/// ⚠ **A filing with no body was never a description of work.** #865 and #866
/// — "MEMORY.md is over its read limit…", filed 16 seconds apart with empty
/// bodies — are this check's own paraphrase fixtures, and shown the corpus
/// unfiltered a real MEMORY.md filing would be advised against them.
#[test]
fn a_closed_row_with_no_body_is_not_read() {
    assert!(!worth_reading(false));
    assert!(worth_reading(true));
}

/// ⚠ **Closing quickly is NOT the signal, and it was nearly the rule.** #863
/// was dropped 58 seconds after filing and is the most valuable row in the
/// closed corpus. Nothing here may reject a row for how fast it closed — the
/// filter takes only whether it says anything.
#[test]
fn the_filter_cannot_see_how_fast_a_task_closed() {
    // The signature is the guard: there is no time to pass in, so no future
    // edit can quietly start rejecting on one.
    assert!(
        worth_reading(true),
        "a row with a body is read whenever it closed"
    );
}

/// An id off the closed list is a closed match, and an id off the open list is
/// an open one — the two arms do different things, so the split is load-bearing.
#[test]
fn matches_are_split_by_which_list_they_came_off() {
    let settled = vec![
        Settled {
            id: 863,
            subject: "MEMORY.md".into(),
            dropped: true,
        },
        Settled {
            id: 689,
            subject: "k8s Dhall".into(),
            dropped: false,
        },
    ];
    let found = vec![
        Match {
            id: 255,
            why: "an open one".into(),
        },
        Match {
            id: 863,
            why: "a dropped one".into(),
        },
    ];
    let (open, over) = split(&found, &settled);
    assert_eq!(
        open,
        vec![Match {
            id: 255,
            why: "an open one".into()
        }]
    );
    assert_eq!(over.len(), 1);
    assert_eq!(over[0].1.id, 863);
    assert!(over[0].1.dropped);
}

/// ⚠ **Nothing that varies per filing may reach this string.** It is put where
/// a cached prefix goes, and a cache block ends where the varying text begins:
/// measured 2026-08-25, the same 995 titles below the subject wrote 32,833
/// tokens and read back zero, every call.
#[test]
fn the_cached_block_carries_the_closed_list_and_no_subject() {
    let settled = vec![
        Settled {
            id: 863,
            subject: "MEMORY.md is 21.7KB".into(),
            dropped: true,
        },
        Settled {
            id: 689,
            subject: "k8s Dhall model".into(),
            dropped: false,
        },
    ];
    let text = settled_block(&settled);
    assert!(
        text.contains("863 | dropped | MEMORY.md is 21.7KB"),
        "{text}"
    );
    assert!(text.contains("689 | done | k8s Dhall model"), "{text}");
    // Twice: once as the thing that must not be here, once as the reason.
    assert!(
        !text.contains("about to be filed:"),
        "no subject in the cached half"
    );
    let same = settled_block(&settled);
    assert_eq!(
        text, same,
        "the same corpus must produce the same bytes, or it never caches"
    );
}

/// The prompt says the closed list exists only when one was actually sent.
#[test]
fn the_question_mentions_closed_tasks_only_when_there_are_some() {
    let corpus = [(1, "something".to_string())];
    assert!(prompt("anything", &corpus, true).contains("CLOSED"));
    assert!(!prompt("anything", &corpus, false).contains("CLOSED"));
}

/// The mirror of [`what_a_filing_waits_for_is_not_shown_to_the_reader`].
///
/// ⚠ **Measured 2026-08-25: #1164 was refused against #986, the task it exists
/// to unblock.** The model's reading was correct — both are about verifying the
/// serial-console method before buying the phones — and the answer was wrong,
/// because a blocker is not a copy. `--blocked-on` already exempted one
/// direction; the filer could declare this edge and the tool could not hear it.
#[test]
fn what_a_filing_unblocks_is_not_shown_to_the_reader_either() {
    let corpus = vec![
        (986, "Buy two SDM845 phones for phonos".to_string()),
        (985, "Something else entirely".to_string()),
    ];
    let shown = edged(&corpus, &[], &[986]);
    assert_eq!(shown, vec![(985, "Something else entirely".to_string())]);
}

/// Both edges at once, and neither end is shown.
#[test]
fn a_filing_may_declare_an_edge_in_each_direction() {
    let corpus = vec![
        (1, "waits for this".to_string()),
        (2, "unblocked by this filing".to_string()),
        (3, "unrelated".to_string()),
    ];
    assert_eq!(
        edged(&corpus, &[1], &[2]),
        vec![(3, "unrelated".to_string())]
    );
}

/// ⚠ **The exact-subject guard is NOT narrowed by either edge.** An identical
/// title is one task however it was ordered, and that half has no error rate.
#[test]
fn an_identical_subject_still_collides_with_what_a_filing_unblocks() {
    let corpus = vec![(42, "Fix the parser".to_string())];
    assert_eq!(same_subject("Fix the parser", &corpus), Some(42));
}
