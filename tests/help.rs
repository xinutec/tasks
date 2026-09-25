//! What `--help` states about the model, which is the only place most sessions
//! will ever read it.
//!
//! ⚠ **Text as a feature, not decoration.** Two facts govern how this tool is
//! used and neither is guessable from the commands: a session never ends, and
//! a holder's open tasks are its future work. Without them a session infers the
//! opposite and prefers whoever is online over whoever owns the work.
//!
//! So these assert the remedy is present, and `--help` is where it has to be:
//! `docs/for-sessions.md` is read once, if at all, and this is free on every
//! `task --help` a confused session runs.

use std::process::Command;

/// The help text with its whitespace collapsed.
///
/// ⚠ **Not cosmetic: clap re-wraps to the terminal width**, so a phrase this
/// file asserts on can be split across a newline by nothing more than where the
/// window edge fell.
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
    // Not a warning on this command: the help says what the command means.
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

/// The words sessions typed that the tool had no answer for.
///
/// Taken from what sessions actually typed: each answered `unrecognized
/// subcommand`, often without clap's "a similar subcommand exists" line, since
/// edit distance finds no neighbour.
#[test]
fn the_verbs_sessions_reach_for_are_the_verbs_that_work() {
    for (typed, real) in [
        ("close", "Mark a task finished"),
        ("update", "Change a task's words"),
        ("rank", "Change a task's words"),
        ("history", "One task, with its prose and its history"),
    ] {
        let text = help(&[typed, "--help"]);
        assert!(
            text.contains(real),
            "`task {typed}` should be `{real}`, and said: {text}"
        );
    }
}

/// A flag naming a field this tool deleted has to say so.
///
/// ⚠ **`unexpected argument '--repo' found` reads as a typo**, and the session
/// leaves believing the field exists.
#[test]
fn a_field_that_was_removed_is_refused_by_name() {
    let out = nameless(&["add", "anything", "--priority", "P2", "--repo", "tumor"]);
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("migration 0004"), "{said}");
    assert!(!out.status.success());
}

#[test]
fn the_subject_is_not_a_flag_and_the_refusal_says_where_it_goes() {
    let out = nameless(&["add", "--priority", "P2", "--subject", "a title"]);
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("first argument"), "{said}");
}

/// The CLI run by something that is not a conversation.
///
/// ⚠ **Clearing the two variables is the whole point.** A scheduler sets
/// neither, and a refusal that needs no identity must not wait on one — see
/// `Client::identified`. Supply a `--session` here and the test stops testing
/// anything.
fn nameless(args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_task"))
        .args(args)
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("TASKS_SESSION")
        .output()
        .expect("running the CLI")
}

/// `--reason`, `--message` and `--note` are accepted; `--body` deliberately is
/// NOT an alias, because everywhere else it means "replace everything".
#[test]
fn closing_a_task_takes_its_outcome_in_the_same_command() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_task"))
        .args(["done", "--help"])
        .output()
        .expect("task done --help");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("--note"), "{text}");
    // ⚠ Matched as an OPTION LINE, not as a substring: the flag's own help
    // explains why `--body` is not an alias, so the word appears in the prose
    // and a `contains` check cannot tell the two apart.
    assert!(
        !text
            .lines()
            .any(|line| line.trim_start().starts_with("--body")),
        "--body must not be an option on done: {text}"
    );
}

/// A drop that records no reason is unreadable later: the status says nothing
/// about why.
#[test]
fn dropping_a_task_takes_its_reason_in_the_same_command() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_task"))
        .args(["drop", "--help"])
        .output()
        .expect("task drop --help");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("--reason"), "{text}");
}
