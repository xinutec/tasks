//! The prompt hook, `task hook`: Claude Code's `UserPromptSubmit` runs it on
//! every prompt, and what it prints joins the conversation. It puts this
//! session's open tasks in front of it, one line each, because the CLI's own
//! task list re-sent every task ever made with full bodies (527 kB a turn on
//! one session). What is printed stays in the context and is re-sent with every
//! turn after it, so it prints only what is news.
//!
//! It asks the service across the VPN, so a digest younger than [`TTL`] is
//! served from a cache, and when the service cannot be reached the last one is
//! the answer: stale beats blocking, and both beat failing. Every write the CLI
//! makes drops the cache ([`forget_digest`]): a session not shown the task it
//! just filed may file it again.
//!
//! The cache ignores `XDG_CACHE_HOME`, as it always has.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How long a cached digest is served without asking. The list changes a few
/// times an hour, and a write drops the cache anyway.
pub const TTL: Duration = Duration::from_secs(60);

/// When a cache served because the service could not be reached is old enough
/// to say so. Under it, a line on every prompt would be the cost this refuses.
pub const STALE_AFTER: Duration = Duration::from_secs(3600);

/// How long an unchanged list may go unrepeated. Compaction removes the earlier
/// copies and cannot be seen from here, so an unchanged digest is printed again
/// after this. Replaying 2,121 real injections: every prompt, 3,995 kB; change
/// or 20 minutes, 2,123 kB; change only, 1,690 kB but a 264-minute p95 gap.
/// Twenty minutes buys back the blind window for 11 of the 58 points.
pub const REPRINT_AFTER: Duration = Duration::from_secs(1200);

/// Where the hook keeps its last answer for a session.
pub fn digest_cache_path(home: &Path, session: &str) -> PathBuf {
    home.join(".cache")
        .join("claude-tasks")
        .join(format!("{session}.txt"))
}

/// The last text actually printed to a session.
pub fn printed_path(home: &Path, session: &str) -> PathBuf {
    home.join(".cache")
        .join("claude-tasks")
        .join(format!("{session}.printed"))
}

/// Drop a session's cached digest. Silent on failure: the write has happened,
/// and the cost is a briefly stale prompt.
pub fn forget_digest(home: &Path, session: &str) {
    let _ = std::fs::remove_file(digest_cache_path(home, session));
}

/// A file's text and age, or `None`.
fn aged(path: &Path) -> Option<(String, Duration)> {
    let text = std::fs::read_to_string(path).ok()?;
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    Some((text, age))
}

/// A failure costs a refetch or a repeat, never the prompt.
fn keep(path: &Path, text: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, text);
}

/// The cached digest when it is young enough to serve without asking.
pub fn fresh(home: &Path, session: &str) -> Option<String> {
    aged(&digest_cache_path(home, session))
        .filter(|(_, age)| *age < TTL)
        .map(|(text, _)| text)
}

/// What to print once the service has been asked: its answer, kept for the
/// next prompt, or else the cache, said to be old past [`STALE_AFTER`], or
/// else nothing. Silence rather than "unreachable" on every prompt: a session
/// told that each time learns to ignore the one line this may spend.
pub fn settle(home: &Path, session: &str, fetched: Option<String>, service: &str) -> String {
    if let Some(text) = fetched {
        keep(&digest_cache_path(home, session), &text);
        return text;
    }
    match aged(&digest_cache_path(home, session)) {
        None => String::new(),
        Some((text, age)) if age > STALE_AFTER => {
            let hours = age.as_secs() / 3600;
            format!("{text}\n⚠ {service} could not be reached; this list is {hours}h old.")
        }
        Some((text, _)) => text,
    }
}

/// Is `body` news: different from what was last printed, or that copy old
/// enough that a compaction may have taken it? Records it when it is.
pub fn worth_printing(home: &Path, session: &str, body: &str) -> bool {
    if body.is_empty() {
        return false;
    }
    let path = printed_path(home, session);
    let news = match aged(&path) {
        Some((text, age)) => text != body || age >= REPRINT_AFTER,
        None => true,
    };
    if news {
        keep(&path, body);
    }
    news
}
