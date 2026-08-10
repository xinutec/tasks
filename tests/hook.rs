//! The half of the prompt hook's contract that lives in this repository.
//!
//! The other half is `xinutec-infra/mac-mini/claude_tasks.py`. Nothing links
//! the two at build time, so these assertions are what stops the CLI clearing a
//! file the hook is not reading.

use std::path::Path;

use tasks::hook::{digest_cache_path, forget_digest};

/// The hook's `CACHE_DIR / f"{session}.txt"`, spelled in Rust.
#[test]
fn the_path_is_the_one_the_hook_reads() {
    let path = digest_cache_path(Path::new("/home/example"), "d5c6955f-a76d");
    assert_eq!(
        path.to_str().expect("utf-8"),
        "/home/example/.cache/claude-tasks/d5c6955f-a76d.txt"
    );
}

#[test]
fn forgetting_removes_the_cached_digest() {
    let home = std::env::temp_dir().join(format!("tasks-hook-{}", std::process::id()));
    let dir = home.join(".cache").join("claude-tasks");
    std::fs::create_dir_all(&dir).expect("a home to work in");
    let path = digest_cache_path(&home, "sess-1");
    std::fs::write(&path, "3 open task(s)").expect("a cached digest");

    forget_digest(&home, "sess-1");
    assert!(!path.exists(), "the stale digest survived the write");

    // Twice, and on a session that never had one: this runs after every write,
    // so its failure mode must be nothing at all.
    forget_digest(&home, "sess-1");
    forget_digest(&home, "never-cached");

    // Explicitly discarded: a leftover temp directory is not worth failing a
    // green test over, and there is nothing to do about it here anyway.
    let _ = std::fs::remove_dir_all(&home);
}
