//! Reading a conversation's name out of the transcript Claude Code writes.
//!
//! ⚠ **The spelling of these lines is a contract with a binary nobody here
//! controls.** `{"type":"agent-name",…}` is emitted by the CLI, and if it ever
//! changes shape this silently stops naming anybody — so the failure has to be
//! *no name* rather than a wrong one, and every assertion below is about which
//! of several candidate names wins rather than about parsing succeeding.
//!
//! The shapes were established in `memview`'s `reader::transcript`, which read
//! them out of the 2.1.221 binary rather than guessing: the labeller's chain is
//! `agentName || customTitle || …`, and that order is what [`in_tail`] follows.

use std::path::Path;

use tasks::agent_name::{TAIL_WINDOW, from_projects, in_tail};

const ME: &str = "2be586d6-c868-4717-8364-7b5b8610abe5";
const OTHER: &str = "7c0202eb-080b-40a5-a654-8758b4ca723e";

fn agent_line(name: &str, session: &str) -> String {
    format!(r#"{{"type":"agent-name","agentName":"{name}","sessionId":"{session}"}}"#)
}

fn title_line(name: &str, session: &str) -> String {
    format!(r#"{{"type":"custom-title","customTitle":"{name}","sessionId":"{session}"}}"#)
}

#[test]
fn the_last_name_wins_because_a_rename_appends_another() {
    // The CLI does not rewrite the earlier line, so a transcript holds every
    // name a conversation has ever gone by. Reading the first one would name a
    // session for the job it had on the day it started.
    let text = [
        agent_line("scanner", ME),
        r#"{"type":"user","message":"…"}"#.to_string(),
        agent_line("tasks", ME),
    ]
    .join("\n");
    assert_eq!(in_tail(text.as_bytes(), ME).as_deref(), Some("tasks"));
}

#[test]
fn a_line_naming_another_session_names_nobody_here() {
    // This is the guard that makes a subagent safe to read: its transcript
    // carries its parent's context, and quoting a name is not being called it.
    let text = agent_line("memview", OTHER);
    assert_eq!(in_tail(text.as_bytes(), ME), None);
}

#[test]
fn a_custom_title_names_a_session_no_agent_line_ever_did() {
    // Not hypothetical: measured 2026-08-10, this is how the `tasks` session
    // itself is named — it carries no `agent-name` line at all, and reading
    // only that one needle reported it as unnamed.
    let text = title_line("tasks", ME);
    assert_eq!(in_tail(text.as_bytes(), ME).as_deref(), Some("tasks"));
}

#[test]
fn the_agent_name_outranks_a_custom_title() {
    // The CLI's own labeller order — `agentName || customTitle` — and the
    // question here is the labeller's: who did this work, not what to call the
    // row in a list somebody picks from.
    let text = [title_line("a title", ME), agent_line("tasks", ME)].join("\n");
    assert_eq!(in_tail(text.as_bytes(), ME).as_deref(), Some("tasks"));

    // And order in the file does not decide it: the title is last here.
    let text = [agent_line("tasks", ME), title_line("a title", ME)].join("\n");
    assert_eq!(in_tail(text.as_bytes(), ME).as_deref(), Some("tasks"));
}

#[test]
fn a_name_printed_inside_a_tool_result_is_not_a_name() {
    // Transcripts on this machine contain their own format, because sessions
    // grep transcripts and the output is filed back. Every quote inside a
    // quoted line is escaped, so anchoring on the unescaped opening is what
    // tells the CLI's own line from a session's `grep` output of one.
    let quoted = format!(
        r#"{{"type":"user","message":"I ran grep and got {}"}}"#,
        agent_line("impostor", ME).replace('"', "\\\"")
    );
    let text = [agent_line("tasks", ME), quoted].join("\n");
    assert_eq!(
        in_tail(text.as_bytes(), ME).as_deref(),
        Some("tasks"),
        "a printed line outranked the CLI's own"
    );
}

#[test]
fn a_name_too_long_to_be_one_is_refused() {
    // A cap rather than trust: this name is rendered in every list and every
    // history row, and the field is whatever the CLI put there.
    let text = agent_line(&"x".repeat(41), ME);
    assert_eq!(in_tail(text.as_bytes(), ME), None);
    let text = agent_line(&"x".repeat(40), ME);
    assert!(in_tail(text.as_bytes(), ME).is_some());
}

#[test]
fn an_empty_name_is_not_a_name() {
    let text = agent_line("", ME);
    assert_eq!(in_tail(text.as_bytes(), ME), None);
}

#[test]
fn nothing_in_the_text_is_no_name_rather_than_an_error() {
    let text = r#"{"type":"user","message":"hello"}"#;
    assert_eq!(in_tail(text.as_bytes(), ME), None);
}

/// The read is bounded, and this is what that costs.
///
/// A transcript on this machine reaches 4.0 GB, so the whole file is not an
/// option on a path that runs before every `task` command. Measured 2026-08-10
/// across the twelve largest: the last name line sits at most 25,376 bytes from
/// the end, and most sit 427. [`TAIL_WINDOW`] is 1 MiB — some forty times the
/// worst case seen — and a name older than that is simply not found.
#[test]
fn only_the_tail_is_looked_at() {
    let mut text = agent_line("ancient", ME).into_bytes();
    text.push(b'\n');
    text.extend(std::iter::repeat_n(b'.', TAIL_WINDOW as usize));
    // The pure function is given only what a tail read would have handed it.
    let tail = &text[text.len() - TAIL_WINDOW as usize..];
    assert_eq!(
        in_tail(tail, ME),
        None,
        "a name outside the window was read"
    );
    assert!(
        in_tail(&text, ME).is_some(),
        "the fixture's name is unreadable even in full, so it proves nothing"
    );
}

#[test]
fn the_transcript_is_found_whichever_project_directory_it_is_under() {
    // A session moves between checkouts, and the directory name encodes the
    // path it was started in — so the file is looked for by id, in all of them.
    let root = std::env::temp_dir().join(format!("tasks-agent-name-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("-Users-pippijn-Code-elsewhere");
    std::fs::create_dir_all(&dir).expect("a projects root to work in");
    std::fs::create_dir_all(root.join("-Users-pippijn-Code")).expect("a second project");
    std::fs::write(dir.join(format!("{ME}.jsonl")), agent_line("tasks", ME)).expect("a transcript");

    assert_eq!(from_projects(&root, ME).as_deref(), Some("tasks"));
    assert_eq!(
        from_projects(&root, OTHER),
        None,
        "named a session with no transcript"
    );
    assert_eq!(
        from_projects(Path::new("/nonexistent/projects"), ME),
        None,
        "a missing projects root should be no name, not a panic"
    );
    let _ = std::fs::remove_dir_all(&root);
}
