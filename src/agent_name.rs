//! What Claude Code calls this conversation, read from its transcript, so a
//! session that never ran `task rename` shows a name rather than a uuid.
//!
//! Claude Code appends `{"type":"agent-name","agentName":"…","sessionId":"…"}`
//! on every rename. Precedence follows its labeller, `agentName` then
//! `customTitle`, as memview's `reader::transcript` does; `ai-title` is a
//! caption, not a name.

use std::path::Path;

/// ⚠ Only the tail is read: transcripts reach gigabytes and this runs before
/// every command. Missing an older name just keeps the current one.
pub const TAIL_WINDOW: u64 = 1 << 20;

/// A name longer than this is not one. It renders in every list.
const MAX_NAME: usize = 40;

/// The lines that carry a name, in the labeller's order.
const NEEDLES: [&str; 2] = [
    r#"{"type":"agent-name","agentName":""#,
    r#"{"type":"custom-title","customTitle":""#,
];

/// A session's transcript, found by trying every directory under `projects/`
/// (their names encode the working directory, undocumented). Used to delete
/// what each one-shot `claude -p` check leaves behind.
pub fn transcript_of(projects_root: &Path, session: &str) -> Option<std::path::PathBuf> {
    std::fs::read_dir(projects_root)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join(format!("{session}.jsonl")))
        .find(|path| path.exists())
}

/// The session's current name, or `None` (unreadable file, changed line shape,
/// a name older than [`TAIL_WINDOW`]), in which case the old name stands.
pub fn from_projects(projects_root: &Path, session: &str) -> Option<String> {
    let Ok(entries) = std::fs::read_dir(projects_root) else {
        return None;
    };
    // Every directory: one encodes where the conversation started, not where it is.
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

/// The name in a stretch of transcript. Apart from the file handling so it can
/// be tested without one.
pub fn in_tail(text: &[u8], session: &str) -> Option<String> {
    NEEDLES
        .iter()
        .find_map(|needle| last_named(text, needle.as_bytes(), session))
}

/// The value on the last line opening with `needle`, if that line is this
/// session's. Matched on the object's whole opening: quoted inside a tool
/// result the quotes are escaped, so only Claude Code's own line matches. The
/// last wins, since a rename appends.
fn last_named(text: &[u8], needle: &[u8], session: &str) -> Option<String> {
    let start = memchr::memmem::rfind(text, needle)? + needle.len();
    let end = start + memchr::memchr(b'"', &text[start..])?;
    let line = end + memchr::memchr(b'\n', &text[end..]).unwrap_or(text.len() - end);
    // The id must be on the same line: a subagent's transcript carries its
    // parent's name too.
    memchr::memmem::find(&text[end..line], session.as_bytes())?;
    let name = std::str::from_utf8(&text[start..end]).ok()?;
    (!name.is_empty() && name.len() <= MAX_NAME).then(|| name.to_string())
}
