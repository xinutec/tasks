//! The prompt hook's cache, seen from the writing end.
//!
//! A session's digest reaches it through `xinutec-infra/mac-mini/claude_tasks.py`,
//! which keeps the last answer in a file and reads it before the network — 60
//! seconds of deliberate staleness, so a burst of prompts costs one round trip
//! rather than one each. That is the right trade for *reading*, and the wrong
//! one for the moment immediately after a write: a session that files a task
//! and is then shown a digest without it in has been given a reason to file it
//! twice, which is the one mistake the pile's visibility exists to prevent.
//!
//! So the CLI drops that file whenever it changes anything. The next prompt
//! pays one request, and sees what this session just did.
//!
//! ⚠ **This is a contract with a file in another repository, and the path is
//! duplicated rather than shared.** There is no import that would carry it from
//! a Python hook to a Rust binary, so the two agree by having been written to;
//! `tests/hook.rs` pins the shape, and the hook's `CACHE_DIR` is the other
//! half. It deliberately does **not** consult `XDG_CACHE_HOME`: the hook builds
//! the path from `Path.home()` alone, and honouring a variable it ignores would
//! make this miss exactly when somebody set one.

use std::path::{Path, PathBuf};

/// Where the hook keeps its last answer for a session.
pub fn digest_cache_path(home: &Path, session: &str) -> PathBuf {
    home.join(".cache")
        .join("claude-tasks")
        .join(format!("{session}.txt"))
}

/// Drop a session's cached digest, so its next prompt asks the service.
///
/// **Best-effort, and silent about it.** The caller has already written; the
/// worst a failure here can do is leave the prompt a minute behind, and
/// reporting it would turn a successful `task add` into something that looks
/// like it failed.
pub fn forget_digest(home: &Path, session: &str) {
    let _ = std::fs::remove_file(digest_cache_path(home, session));
}
