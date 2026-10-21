//! What `P0`–`P4` do to an order, against a real MariaDB.
//!
//! ⚠ The feature is one `COALESCE`, written twice (SQL and [`Priority::rank`]);
//! only a test keeps them level. And unranked is not simply last: `P4` must sink
//! below the unread tasks.

mod common;

use tasks::tasks::repo::{self, Change, Filter, NewTask};
use tasks::tasks::types::{Actor, Moved, Priority, Ranking, Status};

fn human() -> Actor {
    Actor::Person("user".into())
}

fn filed(subject: &str, priority: Option<Priority>) -> NewTask {
    NewTask {
        subject: subject.into(),
        checked: true,
        not_duplicate_of: Vec::new(),
        body: String::new(),
        priority: priority.map_or(Ranking::Unassessed, Ranking::At),
        due: None,
        blocked_on: Vec::new(),
        assignee: None,
        spare: None,
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
        &human(),
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
    // The decision this feature turns on: a DEFAULT would have every unranked
    // row claim a level.
    let pool = common::fresh_db().await;
    let task = repo::create(&pool, filed("Filed the ordinary way", None), &human())
        .await
        .expect("filing");
    assert_eq!(task.priority, None, "an unasked-for rank appeared");
}

/// The ordering end to end, filed in an order that is neither id order nor the
/// answer, so a missing sort can't pass by luck.
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
        repo::create(&pool, filed(subject, priority), &human())
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

/// The Rust and SQL spellings of the rule, compared on one fixture.
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
        repo::create(&pool, filed(&format!("task {n}"), *priority), &human())
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
    let task = repo::create(&pool, filed("Rank me later", None), &human())
        .await
        .expect("filing");

    let updated = repo::update(
        &pool,
        task.id,
        Change {
            priority: Some(Priority::P1),
            ..Default::default()
        },
        &human(),
    )
    .await
    .expect("ranking");
    assert_eq!(updated.task.priority, Some(Priority::P1));
    // ⚠ `Moved::Ranked`, not "priority": the history and this list are one
    // vocabulary.
    assert_eq!(updated.changed, vec![Moved::Ranked]);

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
    let task = repo::create(&pool, filed("Already P2", Some(Priority::P2)), &human())
        .await
        .expect("filing");

    let again = repo::update(
        &pool,
        task.id,
        Change {
            priority: Some(Priority::P2),
            ..Default::default()
        },
        &human(),
    )
    .await
    .expect("re-ranking");
    assert!(again.changed.is_empty(), "{:?}", again.changed);
}

/// ⚠ A rank must not change what counts as open; the clauses sit side by side in one builder.
#[tokio::test]
async fn ranking_changes_the_order_and_nothing_else() {
    let pool = common::fresh_db().await;
    let task = repo::create(&pool, filed("Someday", Some(Priority::P4)), &human())
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
        &human(),
    )
    .await
    .expect("finishing");
    assert!(
        order(&pool).await.is_empty(),
        "a ranked task survived being closed"
    );
}

/// The wire contract: a filer must say, and `null` is a legal answer. Serde
/// tests, because the rule is the shape of `NewTask`, which every client goes
/// through.
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

    /// Everything else stays optional, so the one required field reads as deliberate.
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

/// A bare digit means its level; anything else is still an error, never a default.
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
