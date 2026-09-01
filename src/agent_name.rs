//! What Claude Code calls this conversation, read from its own transcript.
//!
//! ⚠ **A session's name was a self-report, and it did not have to be.** Until
//! this module, `sessions.name` was filled by exactly one thing — `task rename`
//! — so a conversation that never typed it was a 36-character uuid in every
//! list, every history row and every `(from …)` hint, forever. On 2026-08-10 the
//! session holding the most open work of any was one of those: 29 tasks against
//! a raw id, while Claude Code had called it `memview` all along.
//!
//! The name is not a fact only the session knows. The CLI writes
//! `{"type":"agent-name","agentName":"memview","sessionId":"…"}` into the
//! transcript and appends another whenever it changes, so the answer is already
//! on this disk. `README.md` argues that the id is the identity and the name an
//! attribute, which is right and is about *storage*; "so the session pushes the
//! name" was taken to follow from it and does not.
//!
//! **Measured before it was believed** (2026-08-10, all fourteen sessions the
//! service knows): thirteen had a stored name, and all thirteen matched what
//! this module derives. None disagreed. So preferring the derived name changes
//! nothing for the sessions that had typed one and names the one that had not.
//!
//! **The shapes come from `memview`'s `reader::transcript`**, which settled them
//! by reading the 2.1.221 binary rather than by choosing: the labeller's chain
//! is `agentName || customTitle || aiTitle || …`, the resume picker's is a
//! different order, and this is the labeller's question — who did this work.
//! `ai-title` is deliberately not consulted: it is the CLI's own description of
//! a conversation ("Review DICOM scan documentation"), fine as a caption and
//! wrong as a name.

use std::path::Path;

/// How much of the end of a transcript is read.
///
/// ⚠ **This must stay a tail read.** These files reach 4.0 GB on this machine,
/// and this runs before every `task` command. Measured 2026-08-10 across the
/// twelve largest transcripts, the distance from the end of the file to the last
/// name line was at most 25,376 bytes and usually 427 — one carried 3,343 of
/// them, roughly 40 kB apart. A mebibyte is some forty times the worst case
/// seen, and the cost of being wrong is a session that keeps the name it has.
pub const TAIL_WINDOW: u64 = 1 << 20;

/// A name longer than this is not one. It renders in every list.
const MAX_NAME: usize = 40;

/// The line types that carry a name, in the order the CLI's labeller reads them.
const NEEDLES: [&str; 2] = [
    r#"{"type":"agent-name","agentName":""#,
    r#"{"type":"custom-title","customTitle":""#,
];

/// Where a session's transcript is, by id rather than by working out the path.
///
/// ⚠ **The directory under `projects/` is an undocumented encoding of the
/// working directory**, and memview's `past.rs` opens with the account of
/// guessing it wrong. The id is enough: every candidate is tried and the first
/// that exists is the answer.
///
/// This exists for the one-shot sessions `task add`'s duplicate check runs —
/// every `claude -p` call files a transcript, and memview measured that
/// accumulate to 2,299 files and 57 MB in three days when nothing removed them.
/// Naming the session is what makes the leftover findable.
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
    // By id across every project directory, not by the directory a session is
    // sitting in: a conversation spans checkouts, and the directory name encodes
    // the path it was *started* in.
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
/// occurrence, whose session id — are testable without a 4 GB fixture.
pub fn in_tail(text: &[u8], session: &str) -> Option<String> {
    NEEDLES
        .iter()
        .find_map(|needle| last_named(text, needle.as_bytes(), session))
}

/// The value on the last line opening with `needle`, when that line is this
/// session's.
///
/// **Anchored on the whole opening of the object**, not on the field name.
/// Inside a transcript every quote of a quoted line is backslash-escaped, so
/// this exact shape occurs only where the CLI wrote the line itself — never in a
/// tool result that happens to print one, which transcripts on this machine are
/// full of, because sessions grep transcripts and the output is filed back.
///
/// **Last occurrence wins**, which is what makes this current: a rename appends
/// another line rather than rewriting the first, so the newest is the answer and
/// the oldest is the job the conversation had on the day it started.
fn last_named(text: &[u8], needle: &[u8], session: &str) -> Option<String> {
    let start = memchr::memmem::rfind(text, needle)? + needle.len();
    let end = start + memchr::memchr(b'"', &text[start..])?;
    let line = end + memchr::memchr(b'\n', &text[end..]).unwrap_or(text.len() - end);
    // The id sits on the same line, after the name. A line naming another
    // conversation is not this one's name, however it got here — which is what
    // makes a subagent's transcript safe to read: it carries its parent's
    // context, and being quoted is not being called something.
    memchr::memmem::find(&text[end..line], session.as_bytes())?;
    let name = std::str::from_utf8(&text[start..end]).ok()?;
    (!name.is_empty() && name.len() <= MAX_NAME).then(|| name.to_string())
}
