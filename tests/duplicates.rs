//! Reading what a model said about a filing.
//!
//! ⚠ **The strings here are real answers, not invented ones.** Every `said` was
//! produced by the checker model against a live list, replaying tasks whose
//! truth was known — which is where the wrappers come from. A model asked for
//! `#<id> -- <clause>` and nothing else supplies a bullet, a bold marker or a
//! preamble often enough that tolerating them is the feature.
//!
//! Whether the *answer* is right is a property of the prompt and the model,
//! measured by replay rather than asserted here. This file pins that whatever
//! comes back is read correctly.

use tasks::tasks::duplicates::{
    Match, Settled, collision, edged, parse, prompt, refusal, reopen_instead, same_subject,
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
    // Verbatim, including the two-line shape — and both of them wrong.
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
    // ⚠ **Load-bearing, since the answer refuses filings.** A number the model
    // invented — or echoed from the prose it was given — must not be able to
    // block work. The corpus is the only thing that says which ids were real.
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
    // ⚠ **The sentence the measurement turned on.** Without the negative half,
    // a model matches on same area, repo or technology, and most of what it
    // returns is not a duplicate.
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
    // that cannot tell whether the task landed re-runs and makes a real one. And
    // a refusal with no way past it turns a false positive into lost work, when
    // the body is still sitting in the command the caller just ran.
    let text = refusal(&[Match {
        id: 689,
        why: "the same signal.dhall apply".into(),
    }]);
    // ⚠ The verdict lives in the LAST line — see `what_survives_the_tail`. This
    // asserts both halves are present at all.
    assert!(text.contains("NOT FILED"), "{text}");
    assert!(text.contains("--no-duplicate-check"), "{text}");
}

#[test]
fn the_same_subject_twice_is_found_without_a_model() {
    // Identical subjects: the case string equality exists for.
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
    // ⚠ **Only equality refuses here.** Anything looser is the model's
    // question: two tasks that open with the same words are the ordinary case.
    let corpus = [(859, "MEMORY.md is 21.7KB".to_string())];
    assert_eq!(
        same_subject("MEMORY.md is 21.7KB and still growing", &corpus),
        None
    );
}

/// A task cannot be a duplicate of the task it has just declared it waits for.
///
/// ⚠ **The edge is the filer's own statement that these are two different
/// pieces of work.** A check that refuses for a reason the filer has already
/// answered teaches sessions to reach for `--no-duplicate-check` by reflex.
#[test]
fn what_a_filing_waits_for_is_not_shown_to_the_reader() {
    let corpus = vec![
        (984, "Decide the language phonos is written in".to_string()),
        (985, "Something else entirely".to_string()),
    ];
    let shown = edged(&corpus, &[984], &[]);
    assert_eq!(shown, vec![(985, "Something else entirely".to_string())]);
}

#[test]
fn a_filing_that_waits_for_nothing_sees_the_whole_list() {
    let corpus = vec![
        (1, "one".to_string()),
        (2, "two".to_string()),
        (3, "three".to_string()),
    ];
    assert_eq!(edged(&corpus, &[], &[]), corpus);
    // Order is the list's, not the filter's: `parse` reads ids back against
    // this same slice, and a reordered corpus would still be correct but would
    // make a diff of two runs unreadable.
    assert_eq!(
        edged(&corpus, &[2], &[]),
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
/// ⚠ **`reopen` is the whole point**: a session told only that a closed task
/// resembles its filing will file anyway.
///
/// ⚠ **And it must say NOTHING LANDED.** A session told its task exists when it
/// does not has lost the filing and will not come back for it.
#[test]
fn a_closed_match_refuses_and_names_reopen() {
    let text = reopen_instead(
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
        text.contains("NOT FILED"),
        "the caller must know nothing landed: {text}"
    );
    assert!(
        !text.contains("filed anyway"),
        "the old advisory sentence says the opposite of what happens: {text}"
    );
    assert!(
        text.contains("--no-duplicate-check"),
        "a refusal with no way past it turns a false positive into lost work: {text}"
    );
    assert!(
        text.contains("961") && text.contains("34"),
        "the corpus it read: {text}"
    );
}

/// ⚠ **A dropped task is not reported as a decision.** The status carries no
/// reason, and a model asked will invent one — so this line points at the task
/// rather than asserting what its status means.
#[test]
fn a_dropped_match_sends_the_reader_to_the_task_not_to_its_status() {
    let dropped = reopen_instead(
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
    let done = reopen_instead(
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

/// ⚠ **A filing with no body was never a description of work** — typically this
/// check's own paraphrase fixtures, which would otherwise match real filings.
#[test]
fn a_closed_row_with_no_body_is_not_read() {
    assert!(!worth_reading(false));
    assert!(worth_reading(true));
}

/// ⚠ **Closing quickly is NOT the signal**: a row dropped within a minute can
/// carry a complete plan. The filter takes only whether it says anything.
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

/// ⚠ **Nothing that varies per filing may reach this string** — see
/// `duplicates::settled_block`: varying text in the prefix means every call
/// rewrites the cache and reads none of it back.
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
/// ⚠ **A filing resembles the task it unblocks, correctly** — and a blocker is
/// not a copy. `--blocks` lets the filer declare that direction too.
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

/// What a session actually reads of a refusal.
///
/// ⚠ **Sessions pipe this to `tail -3`**, so anything above the last three lines
/// is written for nobody.
///
/// These pin the shape that survives the cut: **one line per finding, and a
/// LAST line that alone says it did not file, why, and what to do.**
mod what_survives_the_tail {
    use super::*;

    /// The last line of anything a refused filing prints.
    fn verdict(text: &str) -> &str {
        text.lines().last().expect("a refusal is never empty")
    }

    #[test]
    fn a_collision_says_everything_in_its_last_line() {
        let text = collision(1206);
        let last = verdict(&text);
        assert!(
            last.contains("NOT FILED"),
            "the verdict is not in it: {last}"
        );
        assert!(
            last.contains("#1206"),
            "what it collided with is lost: {last}"
        );
        assert!(
            last.contains("task edit 1206"),
            "no way to update it: {last}"
        );
        assert!(
            last.contains("--no-duplicate-check"),
            "no way past it: {last}"
        );
    }

    #[test]
    fn a_collision_is_one_line() {
        // ⚠ Not tidiness. A second line pushes the verdict out of a `tail -1`
        // and starts the same erosion this module exists to stop.
        assert_eq!(collision(1206).lines().count(), 1, "{}", collision(1206));
    }

    #[test]
    fn a_model_refusal_says_everything_in_its_last_line() {
        let text = refusal(&[
            Match {
                id: 255,
                why: "the same suspended cron".into(),
            },
            Match {
                id: 689,
                why: "the same signal.dhall apply".into(),
            },
        ]);
        let last = verdict(&text);
        assert!(
            last.contains("NOT FILED"),
            "the verdict is not in it: {last}"
        );
        // ⚠ It must still admit whose opinion it is.
        assert!(
            last.contains("model"),
            "it no longer says who is talking: {last}"
        );
        assert!(
            last.contains("--no-duplicate-check"),
            "no way past it: {last}"
        );
    }

    /// ⚠ **The closed arm's counts ride on its verdict line**: on a line of
    /// their own below it, a `tail -1` would get provenance and no verdict.
    #[test]
    fn a_closed_refusal_says_everything_in_its_last_line() {
        let text = reopen_instead(
            &[(
                Match {
                    id: 689,
                    why: "the same Dhall convergence check".into(),
                },
                Settled {
                    id: 689,
                    subject: "k8s Dhall model apply".into(),
                    dropped: false,
                },
            )],
            984,
            11,
        );
        let last = verdict(&text);
        assert!(
            last.contains("NOT FILED"),
            "the verdict is not in it: {last}"
        );
        // The remedy that differs from the open arm's, which is the only reason
        // this is a second sentence at all.
        assert!(
            last.contains("task reopen"),
            "the move it exists to name is lost: {last}"
        );
        assert!(
            last.contains("model"),
            "it no longer says who is talking: {last}"
        );
        assert!(
            last.contains("--no-duplicate-check"),
            "no way past it: {last}"
        );
        assert!(
            last.contains("984") && last.contains("11"),
            "how much was read fell out of the tail: {last}"
        );
    }

    #[test]
    fn a_closed_refusal_spends_one_line_per_finding_and_one_on_the_verdict() {
        let two: Vec<(Match, Settled)> = (1..=2)
            .map(|n| {
                (
                    Match {
                        id: n,
                        why: format!("finding {n}"),
                    },
                    Settled {
                        id: n,
                        subject: format!("task {n}"),
                        dropped: false,
                    },
                )
            })
            .collect();
        let text = reopen_instead(&two, 10, 0);
        assert_eq!(text.lines().count(), 3, "{text}");
    }

    #[test]
    fn a_refusal_spends_one_line_per_finding_and_one_on_the_verdict() {
        // Three findings is the most the parser keeps, so four lines is the
        // worst case — and `tail -3` still reaches the verdict and two of them.
        let three: Vec<Match> = (1..=3)
            .map(|n| Match {
                id: n,
                why: format!("finding {n}"),
            })
            .collect();
        let text = refusal(&three);
        assert_eq!(text.lines().count(), 4, "{text}");
        let tail: Vec<&str> = text.lines().rev().take(3).collect();
        assert!(
            tail.iter().any(|line| line.contains("NOT FILED")),
            "the verdict fell out of the tail: {text}"
        );
        assert!(
            tail.iter().any(|line| line.contains("#3")),
            "no finding survived beside the verdict: {text}"
        );
    }
}
