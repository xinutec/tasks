//! The prompt hook (`xinutec-infra/mac-mini/claude_tasks.py`) caches each
//! session's digest briefly. After a write that cache is wrong, and a session
//! not shown the task it just filed may file it again, so every write drops it.
//!
//! The path is shared with the hook's `CACHE_DIR` (pinned by `tests/hook.rs`),
//! which ignores `XDG_CACHE_HOME`; so does this.

use std::path::{Path, PathBuf};

/// Where the hook keeps its last answer for a session.
pub fn digest_cache_path(home: &Path, session: &str) -> PathBuf {
    home.join(".cache")
        .join("claude-tasks")
        .join(format!("{session}.txt"))
}

/// Drop a session's cached digest. Silent on failure: the write has happened,
/// and the cost is a briefly stale prompt.
pub fn forget_digest(home: &Path, session: &str) {
    let _ = std::fs::remove_file(digest_cache_path(home, session));
}
