//! The prompt hook's cache: where it lives, what a write drops, and when a
//! digest is served, said to be old, or printed again.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tasks::hook::{
    REPRINT_AFTER, STALE_AFTER, TTL, digest_cache_path, forget_digest, fresh, printed_path, settle,
    worth_printing,
};

/// The path every existing cache is at; moving it would orphan them silently.
#[test]
fn the_path_is_the_one_the_hook_reads() {
    let path = digest_cache_path(Path::new("/home/example"), "d5c6955f-a76d");
    assert_eq!(
        path.to_str().expect("utf-8"),
        "/home/example/.cache/claude-tasks/d5c6955f-a76d.txt"
    );
}

fn home(name: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("tasks-hook-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join(".cache").join("claude-tasks")).expect("a home to work in");
    home
}

fn aged(path: &Path, text: &str, age: Duration) {
    std::fs::write(path, text).expect("written");
    let file = std::fs::File::options()
        .write(true)
        .open(path)
        .expect("opened");
    file.set_modified(SystemTime::now() - age).expect("aged");
}

#[test]
fn forgetting_removes_the_cached_digest() {
    let home = home("forget");
    let path = digest_cache_path(&home, "sess-1");
    std::fs::write(&path, "3 open task(s)").expect("a cached digest");

    forget_digest(&home, "sess-1");
    assert!(!path.exists(), "the stale digest survived the write");

    // Twice, and on a session that never had one: this runs after every write,
    // so its failure mode must be nothing at all.
    forget_digest(&home, "sess-1");
    forget_digest(&home, "never-cached");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn a_young_cache_is_served_and_an_old_one_is_not() {
    let home = home("fresh");
    let path = digest_cache_path(&home, "s");
    aged(&path, "- [ ] #1", Duration::from_secs(5));
    assert_eq!(fresh(&home, "s").as_deref(), Some("- [ ] #1"));
    aged(&path, "- [ ] #1", TTL + Duration::from_secs(1));
    assert_eq!(fresh(&home, "s"), None);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn the_service_answer_is_kept_and_its_absence_falls_back_to_the_cache() {
    let home = home("settle");
    assert_eq!(
        settle(&home, "s", None, "https://t"),
        "",
        "nothing known is silence"
    );
    assert_eq!(
        settle(&home, "s", Some("- [ ] #2".into()), "https://t"),
        "- [ ] #2"
    );
    assert_eq!(fresh(&home, "s").as_deref(), Some("- [ ] #2"));
    assert_eq!(settle(&home, "s", None, "https://t"), "- [ ] #2");
    aged(
        &digest_cache_path(&home, "s"),
        "- [ ] #2",
        STALE_AFTER + Duration::from_secs(3600),
    );
    assert_eq!(
        settle(&home, "s", None, "https://t"),
        "- [ ] #2\n⚠ https://t could not be reached; this list is 2h old."
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn a_digest_is_printed_when_it_is_news() {
    let home = home("printed");
    assert!(
        !worth_printing(&home, "s", ""),
        "an empty digest prints nothing"
    );
    assert!(worth_printing(&home, "s", "a"));
    assert!(
        !worth_printing(&home, "s", "a"),
        "a copy already in the context"
    );
    assert!(worth_printing(&home, "s", "b"));
    aged(&printed_path(&home, "s"), "b", REPRINT_AFTER);
    assert!(
        worth_printing(&home, "s", "b"),
        "a compaction may have taken it"
    );
    let _ = std::fs::remove_dir_all(&home);
}
