//! What `--help` states about the model, which is the only place most sessions
//! will ever read it.
//!
//! ⚠ **Text as a feature, not decoration** — the same argument
//! `tests/digest.rs::the_header_countermands_the_built_in_task_tools` makes. Two
//! facts govern how this tool should be used and neither is guessable from the
//! commands: a session never ends, and a holder's open tasks are its future
//! work. While they were written down nowhere, a session that had used the tool
//! all day inferred the opposite, measured which conversations had live
//! processes, and filed #713 proposing a liveness column and a warning on
//! `move` — both of which would have taught every session to prefer whoever is
//! online over whoever owns the work.
//!
//! So these assert the remedy is present, and `--help` is where it has to be:
//! `docs/for-sessions.md` is read once, if at all, and this is free on every
//! `task --help` a confused session runs.

use std::process::Command;

/// The help text with its whitespace collapsed.
///
/// ⚠ **Not cosmetic: clap re-wraps to the terminal width**, so a phrase this
/// file asserts on can be split across a newline by nothing more than where the
/// window edge fell. Asserting on the raw output failed here for exactly that
/// reason while the sentence was present and correct — a test that would have
/// gone red on somebody else's screen size.
fn help(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_task"))
        .args(args)
        .output()
        .expect("running the CLI");
    assert!(out.status.success(), "`task {args:?}` failed");
    String::from_utf8(out.stdout)
        .expect("utf-8 help")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn the_top_level_help_says_a_session_never_ends() {
    let help = help(&["--help"]);
    assert!(
        help.contains("NEVER ENDS"),
        "the permanence of a session is not stated:\n{help}"
    );
    assert!(
        help.contains("QUEUEING"),
        "nothing says what handing work to a quiet conversation amounts to:\n{help}"
    );
}

#[test]
fn the_top_level_help_says_open_work_is_future_work() {
    let help = help(&["--help"]);
    assert!(
        help.contains("FUTURE WORK"),
        "a holder's open list is left to be read as work in flight:\n{help}"
    );
}

#[test]
fn handing_over_says_to_pick_by_subject_rather_than_by_who_is_awake() {
    // The refused fix, stated as the rule it replaces. #713 proposed warning on
    // this command; the answer was to say what the command means instead.
    let help = help(&["move", "--help"]);
    assert!(
        help.contains("queueing"),
        "`move` does not say that handing to a quiet session is queueing:\n{help}"
    );
    assert!(
        help.contains("whose subject"),
        "`move` does not say what to choose a holder BY:\n{help}"
    );
}

#[test]
fn the_pile_is_not_offered_as_the_cautious_choice() {
    // The failure mode the model prevents: "I am not sure they are still there,
    // so I will put it in the pile" costs every session's prompt instead of one.
    let help = help(&["move", "--help"]);
    assert!(
        help.contains("not the safe default"),
        "nothing warns the pile off being used as a hedge:\n{help}"
    );
}

#[test]
fn editing_states_the_standard_a_body_is_held_to() {
    // ⚠ **The rules are graded against, so they have to be READABLE BEFORE
    // WRITING.** The same three reach a model that reads a body which has grown
    // without being consolidated; stated only there, they arrive after the
    // writing, which is the one moment they cannot prevent anything.
    let help = help(&["edit", "--help"]);
    let collapsed: String = tasks::tasks::density::RUBRIC
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        help.contains(&collapsed),
        "`task edit --help` must print the rubric the judge grades against"
    );
    assert!(
        help.contains("never refuses the write"),
        "a standard that reads as a gate teaches sessions to work around it"
    );
}
