//! The prompt hook's cache, seen from the writing end.
//!
//! A session's digest reaches it through `xinutec-infra/mac-mini/claude_tasks.py`,
//! which serves its last answer from a file for a short while, so a burst of
//! prompts costs one round trip. Right after a write that is wrong: a session
//! shown a digest without the task it just filed has a reason to file it twice.
//! So the CLI drops that file whenever it changes anything.
//!
//! ⚠ **A contract with another repository, and the path is duplicated.** The
//! hook's `CACHE_DIR` is the other half and `tests/hook.rs` pins this one. It
//! deliberately ignores `XDG_CACHE_HOME`, because the hook does.

use std::path::{Path, PathBuf};

/// Where the hook keeps its last answer for a session.
pub fn digest_cache_path(home: &Path, session: &str) -> PathBuf {
    home.join(".cache")
        .join("claude-tasks")
        .join(format!("{session}.txt"))
}

/// Drop a session's cached digest, so its next prompt asks the service.
///
/// **Best-effort and silent**: the write has happened, a failure here only
/// leaves the prompt briefly behind, and reporting it would make a successful
/// `task add` look failed.
pub fn forget_digest(home: &Path, session: &str) {
    let _ = std::fs::remove_file(digest_cache_path(home, session));
}
