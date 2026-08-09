//! What `task list` asks for, and for whom.
//!
//! ⚠ **These pin what the CLI SENDS, and that is only half the property.** A
//! route that accepted `pile=true` and ignored it would keep every one of these
//! green while the pile silently vanished from every session's list. The other
//! half is `tests/digest_route.rs`, which drives `/api/tasks` through the real
//! router; ablating `or_unheld` there fails exactly one test and leaves these
//! five untouched, which is the point of having both.

use tasks::tasks::selection::list_query;

/// The query as `a=b&c=d`, so a test reads like the URL it produces.
fn query(all: bool, mine: bool, done: bool, session: Option<&str>) -> String {
    list_query(all, mine, done, session)
        .expect("a query")
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

#[test]
fn a_bare_list_is_about_the_caller() {
    // The whole point. Before this, a bare `task list` sent no session at all
    // and the service answered with every open task there is — 135 lines,
    // 12,804 bytes, measured — into the context of a conversation that could
    // act on one of them.
    assert_eq!(
        query(false, false, false, Some("sess-1")),
        "session=sess-1&pile=true"
    );
}

#[test]
fn the_pile_comes_with_it() {
    // Not a detail: the pile is how a task reaches whichever conversation is
    // around rather than a named one. A default of strictly-mine would be
    // smaller and would make handover invisible to everybody at once, which is
    // the argument `digest.rs` makes at length for the same rule.
    let bare = query(false, false, false, Some("sess-1"));
    assert!(bare.contains("pile=true"), "{bare}");
    assert!(!query(false, true, false, Some("sess-1")).contains("pile"));
}

#[test]
fn mine_is_strictly_mine_and_all_is_everything() {
    assert_eq!(query(false, true, false, Some("sess-1")), "session=sess-1");
    // `--all` must not narrow by session even though one is known — it is the
    // one way left to ask what the fleet is doing.
    assert_eq!(query(true, false, false, Some("sess-1")), "");
}

#[test]
fn done_rides_along_with_every_selection() {
    for (all, mine) in [(false, false), (false, true), (true, false)] {
        let q = query(all, mine, true, Some("sess-1"));
        assert!(q.contains("done=true"), "all={all} mine={mine}: {q}");
    }
}

#[test]
fn without_an_id_there_is_no_own_to_narrow_to() {
    // Everything, rather than an empty list: a caller with no identity has no
    // plate, and answering "nothing open" would be a lie about the service
    // rather than an answer about them.
    assert_eq!(query(false, false, false, None), "");
    // `--mine`, though, was asked a question that needs one.
    assert!(list_query(false, true, false, None).is_err());
}
