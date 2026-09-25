//! What Claude Code calls this conversation, read from its own transcript.
//!
//! Otherwise `sessions.name` is filled only by `task rename`, and a
//! conversation that never ran it is a uuid in every list and history row. The
//! CLI already writes `{"type":"agent-name","agentName":"…","sessionId":"…"}`
//! into the transcript and appends another on every rename. The id stays the
//! identity; the name is an attribute read from here.
//!
//! The line shapes and their precedence follow the CLI's labeller —
//! `agentName`, then `customTitle` — as memview's `reader::transcript` reads
//! them. `ai-title` is not consulted: it is the CLI's caption for the
//! conversation, not a name.

use std::path::Path;

/// How much of the end of a transcript is read.
///
/// ⚠ **This must stay a tail read**: transcripts reach gigabytes and this runs
/// before every `task` command. The last name line sits far inside the last
/// mebibyte, and missing it only means the session keeps the name it has.
pub const TAIL_WINDOW: u64 = 1 << 20;

/// A name longer than this is not one. It renders in every list.
const MAX_NAME: usize = 40;

/// The line types that carry a name, in the order the CLI's labeller reads them.
const NEEDLES: [&str; 2] = [
    r#"{"type":"agent-name","agentName":""#,
    r#"{"type":"custom-title","customTitle":""#,
];

/// Where a session's transcript is, found by id.
///
/// ⚠ The directory under `projects/` is an undocumented encoding of the working
/// directory, so every directory is tried rather than the path worked out.
///
/// Used to remove the transcript each one-shot `claude -p` check leaves
/// behind; nothing else would, and they accumulate fast.
pub fn transcript_of(projects_root: &Path, session: &str) -> Option<std::path::PathBuf> {
    std::fs::read_dir(projects_root)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join(format!("{session}.jsonl")))
        .find(|path| path.exists())
}

/// The name a session goes by now, from the tail of its own transcript.
///
/// `None` is the whole failure mode: an unreadable file, a CLI that has changed
/// the shape of the line, a name older than [`TAIL_WINDOW`]. The caller keeps
/// whatever name the service already had, which is the behaviour that existed
/// before this module.
pub fn from_projects(projects_root: &Path, session: &str) -> Option<String> {
    let Ok(entries) = std::fs::read_dir(projects_root) else {
        return None;
    };
    // Every project directory, not the current one: a conversation spans
    // checkouts, and the directory encodes where it was *started*.
    for entry in entries.flatten() {
        let path = entry.path().join(format!("{session}.jsonl"));
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        let Ok(meta) = file.metadata() else { continue };
        if let Some(name) = read_tail(&file, meta.len()).and_then(|buf| in_tail(&buf, session)) {
            return Some(name);
        }
    }
    None
}

fn read_tail(file: &std::fs::File, len: u64) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = file;
    let from = len.saturating_sub(TAIL_WINDOW);
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut buf = Vec::with_capacity((len - from) as usize);
    file.take(TAIL_WINDOW).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// The name in a stretch of transcript, or `None`.
///
/// Separate from the file handling so the decisions — which needle wins, which
/// occurrence, whose session id — are testable without a file.
pub fn in_tail(text: &[u8], session: &str) -> Option<String> {
    NEEDLES
        .iter()
        .find_map(|needle| last_named(text, needle.as_bytes(), session))
}

/// The value on the last line opening with `needle`, when that line is this
/// session's.
///
/// **Anchored on the whole opening of the object**, not the field name: a line
/// quoted inside a tool result has its quotes backslash-escaped, so this exact
/// shape occurs only where the CLI wrote it.
///
/// **Last occurrence wins**: a rename appends a line, so the newest is current.
fn last_named(text: &[u8], needle: &[u8], session: &str) -> Option<String> {
    let start = memchr::memmem::rfind(text, needle)? + needle.len();
    let end = start + memchr::memchr(b'"', &text[start..])?;
    let line = end + memchr::memchr(b'\n', &text[end..]).unwrap_or(text.len() - end);
    // The id follows on the same line. A line naming another conversation —
    // a subagent's transcript carries its parent's — is not this one's name.
    memchr::memmem::find(&text[end..line], session.as_bytes())?;
    let name = std::str::from_utf8(&text[start..end]).ok()?;
    (!name.is_empty() && name.len() <= MAX_NAME).then(|| name.to_string())
}
