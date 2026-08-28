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
///
/// `--pile` is not a parameter here: it is a fourth question rather than a
/// modifier of these three, and its tests spell it out with [`pile_query`].
fn query(all: bool, mine: bool, done: bool, session: Option<&str>) -> String {
    joined(list_query(all, mine, false, false, done, session, None).expect("a query"))
}

fn pile_query(done: bool, session: Option<&str>) -> String {
    joined(list_query(false, false, true, false, done, session, None).expect("a query"))
}

fn joined(query: Vec<(String, String)>) -> String {
    query
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
    assert!(list_query(false, true, false, false, false, None, None).is_err());
}

/// The fourth question, which had no name until 2026-08-10.
///
/// ⚠ **A view that does not exist is answered anyway.** Wanting to know what
/// was going spare, a session filtered `--all --json` by hand on a field it
/// guessed — `session`, which is not in the shape — and every row matched,
/// so it reported **137** in the pile against a real **5**, to Pippijn, before
/// anybody checked. The flag is the fix; that number is why it is worth one.
#[test]
fn the_pile_can_be_asked_for_on_its_own() {
    assert_eq!(pile_query(false, Some("sess-1")), "unheld=true");
}

#[test]
fn asking_for_the_pile_does_not_narrow_it_to_a_holder() {
    // The pile has no holder, so an id sent alongside would ask for the
    // intersection of two disjoint sets — reliably nothing, which reads as an
    // empty pile rather than as a query that cannot match.
    assert!(
        !pile_query(false, Some("sess-1")).contains("session"),
        "a session id narrowed a question that is not about holders"
    );
    // And it needs none: this is the one list a person can ask for the same
    // way a session does.
    assert_eq!(pile_query(false, None), "unheld=true");
}

#[test]
fn the_pile_can_include_what_was_closed_out_of_it() {
    // `--done` is orthogonal to which question is being asked, and stays so.
    assert_eq!(pile_query(true, Some("sess-1")), "done=true&unheld=true");
}

#[test]
fn the_pile_is_not_the_widening_that_comes_with_a_bare_list() {
    // Two parameters with two meanings: `pile=true` WIDENS a session's plate,
    // `unheld=true` NARROWS to the unheld. Sending the first for `--pile`
    // would answer with the caller's own work as well, which is the bare list.
    let bare = query(false, false, false, Some("sess-1"));
    let pile = pile_query(false, Some("sess-1"));
    assert!(
        bare.contains("pile=true") && !bare.contains("unheld"),
        "{bare}"
    );
    assert!(
        pile.contains("unheld=true") && !pile.contains("pile=true"),
        "{pile}"
    );
}

/// ⚠ **A person and a session are different COLUMNS**, and getting that wrong
/// answers with silence rather than an error: `person=hardware` matches nothing
/// because `hardware` is a session. Three attempts in the transcripts —
/// `--to pippijn`, `--assignee hardware`, `--pippijn` — and the tool answered
/// none of them.
#[test]
fn asking_for_one_holder_asks_the_right_column() {
    use tasks::tasks::selection::Holder;
    let person = list_query(
        false,
        false,
        false,
        false,
        false,
        None,
        Some(Holder::Person("pippijn")),
    )
    .expect("a query");
    assert_eq!(joined(person), "person=pippijn");
    let session = list_query(
        false,
        false,
        false,
        false,
        false,
        None,
        Some(Holder::Session("abc-123")),
    )
    .expect("a query");
    assert_eq!(joined(session), "session=abc-123");
}

/// ⚠ **No pile.** `--to` answers what one holder is CARRYING, and the unheld
/// tasks belong to nobody — folding them in would answer a different question.
/// The bare `task list` deliberately does fold them in, which is why this is a
/// separate flag rather than a filter on that.
#[test]
fn one_holders_list_does_not_carry_the_pile() {
    use tasks::tasks::selection::Holder;
    let q = joined(
        list_query(
            false,
            false,
            false,
            false,
            false,
            Some("mine"),
            Some(Holder::Session("abc-123")),
        )
        .expect("a query"),
    );
    assert!(q.contains("session=abc-123"), "{q}");
    assert!(!q.contains("pile"), "{q}");
}

/// The pile IS a holder in this vocabulary, so asking for it by name works.
#[test]
fn nobody_is_a_holder_you_can_ask_about() {
    use tasks::tasks::selection::Holder;
    let q = joined(
        list_query(
            false,
            false,
            false,
            false,
            false,
            None,
            Some(Holder::Nobody),
        )
        .expect("a query"),
    );
    assert_eq!(q, "unheld=true");
}

fn handed_out_query(done: bool, session: Option<&str>) -> String {
    joined(list_query(false, false, false, true, done, session, None).expect("a query"))
}

/// The fifth question, and the only one that is not about the holder.
///
/// ⚠ **A routing session cannot see the work it routes.** The digest shows a
/// session its own tasks and the pile and never another conversation's — which
/// is right, and means every task handed to `memview` leaves the router's sight
/// the moment it is assigned. `--to memview` answers what memview carries from
/// ALL filers; nothing answered what this session put there.
#[test]
fn what_i_handed_out_is_asked_for_by_filer_not_by_holder() {
    assert_eq!(handed_out_query(false, Some("sess-1")), "handed_out=sess-1");
}

#[test]
fn handing_out_is_not_the_same_question_as_holding() {
    // `session=` is "what is on my plate" and `handed_out=` is its complement:
    // sending the first for this would answer with exactly the tasks it exists
    // to exclude.
    let handed = handed_out_query(false, Some("sess-1"));
    assert!(!handed.contains("session=sess-1"), "{handed}");
    assert!(!handed.contains("pile"), "{handed}");
    assert!(!handed.contains("unheld"), "{handed}");
}

#[test]
fn what_i_handed_out_needs_to_know_who_i_am() {
    // Unlike `--pile`, this has no answer without an identity: there is no
    // "out" without a "from". An empty list would read as "nothing outstanding".
    assert!(list_query(false, false, false, true, false, None, None).is_err());
}

#[test]
fn handed_out_carries_done_like_every_other_selection() {
    // Which is the useful one for a router: what it handed out and got back.
    assert_eq!(
        handed_out_query(true, Some("sess-1")),
        "done=true&handed_out=sess-1"
    );
}
