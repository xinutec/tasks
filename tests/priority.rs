//! What `P0`–`P4` do to an order, against a real MariaDB.
//!
//! ⚠ **The whole feature is one `COALESCE`, and it is written twice.** The SQL
//! sorts on `COALESCE(t.priority, 'P2')` and Rust answers the same question in
//! [`Priority::rank`]; they were written on the same afternoon and nothing but a
//! test makes them stay level. A drift between them is silent — every list still
//! returns every task, in an order nobody notices is wrong until the thing they
//! ranked `P0` is not at the top.
//!
//! ⚠ **The second property is the one that is easy to get backwards**, and it is
//! why unranked is not simply sorted last: ranking a task `P4` must push it
//! DOWN, past the tasks nobody has read. "Ranked first, unranked after" would
//! have lifted *when there is room* above four hundred untriaged tickets, which
//! is the opposite of what the word means.

mod common;

use tasks::tasks::repo::{self, Change, Filter, NewTask};
use tasks::tasks::types::{Actor, Priority, Ranking, Status};

fn pippijn() -> Actor {
    Actor::Person("pippijn".into())
}

fn filed(subject: &str, priority: Option<Priority>) -> NewTask {
    NewTask {
        subject: subject.into(),
        body: String::new(),
        priority: priority.map_or(Ranking::Unassessed, Ranking::At),
        due: None,
        blocked_on: Vec::new(),
        assignee: None,
    }
}

/// Every open task's subject, in the order the service answers with.
async fn order(pool: &sqlx::MySqlPool) -> Vec<String> {
    repo::list(pool, &Filter::default())
        .await
        .expect("listing")
        .into_iter()
        .map(|t| t.subject)
        .collect()
}

#[tokio::test]
async fn a_rank_is_stored_and_comes_back() {
    let pool = common::fresh_db().await;
    let task = repo::create(
        &pool,
        filed("Ranked when filed", Some(Priority::P0)),
        &pippijn(),
    )
    .await
    .expect("filing");
    assert_eq!(task.priority, Some(Priority::P0));

    // Read back through the list projection too: the column is selected in a
    // `concat!` literal, and a projection that forgot it would still compile.
    let listed = repo::list(&pool, &Filter::default())
        .await
        .expect("listing");
    assert_eq!(listed[0].priority, Some(Priority::P0));
}

#[tokio::test]
async fn nothing_is_ranked_unless_somebody_ranks_it() {
    // The decision this feature turns on. 700-odd rows existed when the column
    // was added; a DEFAULT would have had every one of them claim a level.
    let pool = common::fresh_db().await;
    let task = repo::create(&pool, filed("Filed the ordinary way", None), &pippijn())
        .await
        .expect("filing");
    assert_eq!(task.priority, None, "an unasked-for rank appeared");
}

/// The ordering, end to end, with the two directions in one fixture.
///
/// Filed in an order that is neither the id order nor the answer, so a `list`
/// that forgot to sort at all, or sorted only by id, fails rather than passing
/// by luck.
#[tokio::test]
async fn ranks_rise_and_sink_around_the_untriaged() {
    let pool = common::fresh_db().await;
    for (subject, priority) in [
        ("first filed, untouched", None),
        ("second filed, untouched", None),
        ("filed late, urgent", Some(Priority::P0)),
        ("filed early-ish, someday", Some(Priority::P4)),
        ("filed last, next", Some(Priority::P1)),
        ("third filed, untouched", None),
    ] {
        repo::create(&pool, filed(subject, priority), &pippijn())
            .await
            .expect("filing");
    }

    assert_eq!(
        order(&pool).await,
        vec![
            "filed late, urgent",       // P0
            "filed last, next",         // P1
            "first filed, untouched",   // unranked, and still oldest first
            "second filed, untouched",  //
            "third filed, untouched",   //
            "filed early-ish, someday", // P4, BELOW everything nobody has read
        ]
    );
}

/// The two spellings of the same rule, compared rather than assumed.
///
/// `Priority::rank` is the Rust one and `COALESCE(priority, 'P2')` is the SQL
/// one. This sorts one fixture with each and asserts they agree, so a change to
/// either that is not made to the other fails here instead of in a list nobody
/// is checking.
#[tokio::test]
async fn the_sql_order_is_the_one_rust_describes() {
    let pool = common::fresh_db().await;
    let fixture = [
        Some(Priority::P3),
        None,
        Some(Priority::P0),
        Some(Priority::P2),
        Some(Priority::P4),
        None,
        Some(Priority::P1),
    ];
    for (n, priority) in fixture.iter().enumerate() {
        repo::create(&pool, filed(&format!("task {n}"), *priority), &pippijn())
            .await
            .expect("filing");
    }

    let mut expected: Vec<String> = (0..fixture.len()).map(|n| format!("task {n}")).collect();
    // Stable, so equal ranks keep the order they were filed in — which is id
    // order, and is the tiebreak the SQL spells as `, t.id`.
    expected.sort_by_key(|subject| {
        let n: usize = subject.trim_start_matches("task ").parse().unwrap();
        Priority::rank(fixture[n])
    });

    assert_eq!(order(&pool).await, expected);
}

#[tokio::test]
async fn ranking_a_task_afterwards_says_so_in_its_history() {
    let pool = common::fresh_db().await;
    let task = repo::create(&pool, filed("Rank me later", None), &pippijn())
        .await
        .expect("filing");

    let updated = repo::update(
        &pool,
        task.id,
        Change {
            priority: Some(Priority::P1),
            ..Default::default()
        },
        &pippijn(),
    )
    .await
    .expect("ranking");
    assert_eq!(updated.task.priority, Some(Priority::P1));
    assert_eq!(updated.changed, vec!["priority"]);

    let detail = repo::get(&pool, task.id)
        .await
        .expect("reading")
        .expect("a task");
    let ranked: Vec<&str> = detail
        .events
        .iter()
        .filter(|e| e.kind == "ranked")
        .filter_map(|e| e.detail.as_deref())
        .collect();
    // "unranked" spelled out rather than left blank: the line has to say which
    // direction it went, and an arrow with nothing on its left says nothing.
    assert_eq!(ranked, vec!["unranked → P1"]);
}

#[tokio::test]
async fn re_ranking_a_task_to_what_it_already_is_writes_no_history() {
    // The rule the rest of `update` follows: a change that changes nothing is
    // not an event, because a history full of non-events is one nobody reads.
    let pool = common::fresh_db().await;
    let task = repo::create(&pool, filed("Already P2", Some(Priority::P2)), &pippijn())
        .await
        .expect("filing");

    let again = repo::update(
        &pool,
        task.id,
        Change {
            priority: Some(Priority::P2),
            ..Default::default()
        },
        &pippijn(),
    )
    .await
    .expect("re-ranking");
    assert!(again.changed.is_empty(), "{:?}", again.changed);
}

/// ⚠ **A rank must not leak into what counts as open.**
///
/// `still_open!` is the only place the open/closed vocabulary is spelled in SQL,
/// and the ordering clause sits beside it in the same builder. A `P4` task is
/// still work; a closed one is still closed whatever it is ranked.
#[tokio::test]
async fn ranking_changes_the_order_and_nothing_else() {
    let pool = common::fresh_db().await;
    let task = repo::create(&pool, filed("Someday", Some(Priority::P4)), &pippijn())
        .await
        .expect("filing");
    assert_eq!(order(&pool).await.len(), 1, "a P4 task left the open list");

    repo::update(
        &pool,
        task.id,
        Change {
            status: Some(Status::Done),
            ..Default::default()
        },
        &pippijn(),
    )
    .await
    .expect("finishing");
    assert!(
        order(&pool).await.is_empty(),
        "a ranked task survived being closed"
    );
}

/// The wire contract: a filer must SAY, and `null` is a legal thing to say.
///
/// ⚠ **These are serde tests on purpose.** The rule lives in the shape of
/// `NewTask`, not in a runtime check inside `create`, so the thing to pin is
/// what the deserialiser accepts — every client reaches the service through it,
/// and a rule enforced in one client is not a rule.
mod filing {
    use tasks::tasks::repo::NewTask;
    use tasks::tasks::types::{Priority, Ranking};

    /// The ablation for this whole module. Restore `#[serde(default)]` on
    /// `NewTask::priority` and only this test fails — which is what says the
    /// attribute is load-bearing rather than decoration.
    #[test]
    fn omitting_priority_is_refused() {
        let err = serde_json::from_str::<NewTask>(r#"{"subject":"Something"}"#)
            .expect_err("a filing that never mentioned priority was accepted");
        assert!(
            err.to_string().contains("priority"),
            "the refusal has to name the field a filer left out, got: {err}"
        );
    }

    /// Explicit null is how a filer says "I am not judging this one".
    #[test]
    fn null_is_unassessed_and_is_accepted() {
        let new: NewTask = serde_json::from_str(r#"{"subject":"Something","priority":null}"#)
            .expect("an explicit `unassessed` filing was refused");
        assert_eq!(
            new.priority,
            Ranking::Unassessed,
            "null should mean unassessed"
        );
    }

    #[test]
    fn a_level_is_carried_through() {
        let new: NewTask = serde_json::from_str(r#"{"subject":"Something","priority":"P1"}"#)
            .expect("a ranked filing was refused");
        assert_eq!(new.priority, Ranking::At(Priority::P1));
    }

    /// Absence still means *leave it alone* everywhere else, and that contrast
    /// is the reason the rule is legible: `body`, `due`, `blocked_on` and
    /// `assignee` all stay optional, so the one required field reads as a
    /// deliberate exception rather than an inconsistency.
    #[test]
    fn every_other_field_is_still_optional() {
        let new: NewTask = serde_json::from_str(r#"{"subject":"Something","priority":null}"#)
            .expect("filing with only the two required fields");
        assert!(new.body.is_empty());
        assert_eq!(new.due, None);
        assert!(new.blocked_on.is_empty());
        assert!(new.assignee.is_none());
    }
}

/// What a session types when it means P3.
///
/// ⚠ **Five filings across the transcripts passed `--priority 3` and were
/// refused** (#958). A bare digit has no other meaning at this flag, and the
/// stored spelling stays `P3` — so accepting it costs nothing and saves a
/// re-run. Anything that is not a level is still an error, which is the half
/// that must not drift: a silent default here would file work at a rank nobody
/// chose.
#[test]
fn a_bare_digit_is_the_level_a_session_meant() {
    use std::str::FromStr;
    for (typed, want) in [
        ("3", Priority::P3),
        ("P3", Priority::P3),
        ("p3", Priority::P3),
        ("0", Priority::P0),
        ("4", Priority::P4),
    ] {
        assert_eq!(Priority::from_str(typed).expect(typed), want, "{typed}");
    }
    for nonsense in ["5", "P5", "high", "", "-1"] {
        assert!(Priority::from_str(nonsense).is_err(), "{nonsense}");
    }
}
