//! The SQL, against a real MariaDB.
//!
//! The queries here are runtime strings, so running them is the only check on
//! them — a renamed column compiles perfectly and fails on the first request.

mod common;

use tasks::sessions;
use tasks::tasks::repo::{self, Change, Filter, NewTask};
use tasks::tasks::types::{Actor, Assignee, AssigneeKind, Status};

fn pippijn() -> Actor {
    Actor::Person("pippijn".into())
}

fn to_person() -> Assignee {
    Assignee {
        kind: AssigneeKind::Person,
        id: Some("pippijn".into()),
        name: None,
    }
}

fn to_session(id: &str) -> Assignee {
    Assignee {
        kind: AssigneeKind::Session,
        id: Some(id.to_string()),
        name: None,
    }
}

fn filed(repo_name: &str, subject: &str) -> NewTask {
    NewTask {
        repo: Some(repo_name.into()),
        subject: subject.into(),
        body: String::new(),
        assignee: None,
    }
}

#[tokio::test]
async fn a_filed_task_comes_back_open_and_in_the_pile() {
    let pool = common::fresh_db().await;
    let task = repo::create(&pool, filed("memview", "Something to do"), &pippijn())
        .await
        .expect("filing");
    assert_eq!(task.subject, "Something to do");
    assert_eq!(task.status, Status::Open);
    assert_eq!(task.assignee.kind, AssigneeKind::Nobody);
    assert!(!task.detailed, "no body was given");

    let listed = repo::list(&pool, &Filter::default())
        .await
        .expect("listing");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, task.id);
}

#[tokio::test]
async fn a_finished_task_leaves_every_open_list_and_stays_in_the_record() {
    let pool = common::fresh_db().await;
    let task = repo::create(&pool, filed("memview", "Finish me"), &pippijn())
        .await
        .expect("filing");

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

    let open = repo::list(&pool, &Filter::default())
        .await
        .expect("listing");
    assert!(open.is_empty(), "a done task is still in the open list");

    // The point of keeping it: the file scheme deleted this and relied on git.
    let all = repo::list(
        &pool,
        &Filter {
            include_done: true,
            ..Default::default()
        },
    )
    .await
    .expect("listing everything");
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].status, Status::Done);
    assert!(all[0].closed_at.is_some(), "no closing time was recorded");
}

#[tokio::test]
async fn moving_a_task_between_the_two_of_us_is_recorded_both_ways() {
    let pool = common::fresh_db().await;
    sessions::touch(&pool, "sess-1", Some("memview"))
        .await
        .expect("recording a session");
    let task = repo::create(&pool, filed("memview", "Hand this over"), &pippijn())
        .await
        .expect("filing");

    let held = repo::update(
        &pool,
        task.id,
        Change {
            assignee: Some(to_session("sess-1")),
            ..Default::default()
        },
        &pippijn(),
    )
    .await
    .expect("handing over");
    assert_eq!(held.assignee.kind, AssigneeKind::Session);
    assert_eq!(held.assignee.id.as_deref(), Some("sess-1"));
    // Resolved through the join, so a list draws a name without a second query.
    assert_eq!(held.assignee.name.as_deref(), Some("memview"));

    let back = repo::update(
        &pool,
        task.id,
        Change {
            assignee: Some(to_person()),
            ..Default::default()
        },
        &Actor::Session("sess-1".into()),
    )
    .await
    .expect("handing back");
    assert_eq!(back.assignee.kind, AssigneeKind::Person);

    let detail = repo::get(&pool, task.id)
        .await
        .expect("reading")
        .expect("a task");
    let moves: Vec<&str> = detail
        .events
        .iter()
        .filter(|e| e.kind == "assigned")
        .filter_map(|e| e.detail.as_deref())
        .collect();
    assert_eq!(moves, vec!["nobody → memview", "memview → pippijn"]);
    // Who did it comes from the credential, so the two moves have two actors —
    // and the session is named, not printed as its id.
    let actors: Vec<&str> = detail
        .events
        .iter()
        .filter(|e| e.kind == "assigned")
        .map(|e| e.actor.as_str())
        .collect();
    assert_eq!(actors, vec!["pippijn", "memview"]);
}

#[tokio::test]
async fn history_names_the_session_that_acted_rather_than_its_id() {
    // The same session read as `memview` in one column and as a 36-character id
    // in the next, on the same line, until the actor was resolved too.
    let pool = common::fresh_db().await;
    sessions::touch(&pool, "sess-1", Some("memview"))
        .await
        .expect("recording");
    let task = repo::create(
        &pool,
        filed("memview", "Acted on"),
        &Actor::Session("sess-1".into()),
    )
    .await
    .expect("filing");

    let detail = repo::get(&pool, task.id)
        .await
        .expect("reading")
        .expect("a task");
    assert_eq!(detail.events[0].actor, "memview");

    // A session nobody has named still says something, and the something is its
    // id rather than a blank.
    let anon = repo::create(
        &pool,
        filed("memview", "By a stranger"),
        &Actor::Session("sess-9".into()),
    )
    .await
    .expect("filing");
    let detail = repo::get(&pool, anon.id)
        .await
        .expect("reading")
        .expect("a task");
    assert_eq!(detail.events[0].actor, "sess-9");
}

#[tokio::test]
async fn a_change_that_changes_nothing_writes_no_history() {
    let pool = common::fresh_db().await;
    let task = repo::create(&pool, filed("memview", "Steady"), &pippijn())
        .await
        .expect("filing");

    for _ in 0..3 {
        repo::update(
            &pool,
            task.id,
            Change {
                subject: Some("Steady".into()),
                status: Some(Status::Open),
                assignee: Some(Assignee::nobody()),
                ..Default::default()
            },
            &pippijn(),
        )
        .await
        .expect("re-stating the same thing");
    }

    let detail = repo::get(&pool, task.id)
        .await
        .expect("reading")
        .expect("a task");
    assert_eq!(
        detail.events.len(),
        1,
        "a client restating the object filled the history: {:?}",
        detail.events
    );
}

#[tokio::test]
async fn a_subject_is_one_line_and_a_body_is_not_in_the_list() {
    let pool = common::fresh_db().await;

    let long = "x".repeat(400);
    let refused = repo::create(&pool, filed("memview", &long), &pippijn()).await;
    assert!(refused.is_err(), "a 400-character subject was accepted");

    let split = repo::create(&pool, filed("memview", "one\ntwo"), &pippijn()).await;
    assert!(split.is_err(), "a two-line subject was accepted");

    let task = repo::create(
        &pool,
        NewTask {
            repo: Some("memview".into()),
            subject: "Has prose".into(),
            body: "# Why\n\nA paragraph of reasoning.".into(),
            assignee: None,
        },
        &pippijn(),
    )
    .await
    .expect("filing");
    assert!(task.detailed, "the body was not noticed");

    // ⚠ The listing type has no body field at all — this is the property, and
    // it is enforced by the type rather than by remembering not to select it.
    let listed = repo::list(&pool, &Filter::default())
        .await
        .expect("listing");
    let json = serde_json::to_string(&listed).expect("serialising");
    assert!(
        !json.contains("A paragraph of reasoning"),
        "a list carried a body: {json}"
    );
    assert!(
        json.contains("Has prose"),
        "a list lost its subject: {json}"
    );
}

#[tokio::test]
async fn a_session_rename_moves_no_task() {
    let pool = common::fresh_db().await;
    sessions::touch(&pool, "sess-1", Some("memview"))
        .await
        .expect("recording");
    let task = repo::create(
        &pool,
        NewTask {
            repo: Some("memview".into()),
            subject: "Assigned to a session that will be renamed".into(),
            body: String::new(),
            assignee: Some(to_session("sess-1")),
        },
        &pippijn(),
    )
    .await
    .expect("filing");

    sessions::touch(&pool, "sess-1", Some("tasks"))
        .await
        .expect("renaming");

    let after = repo::list(&pool, &Filter::default())
        .await
        .expect("listing");
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id, task.id);
    assert_eq!(after[0].assignee.id.as_deref(), Some("sess-1"));
    assert_eq!(after[0].assignee.name.as_deref(), Some("tasks"));
}

#[tokio::test]
async fn touching_a_session_without_a_name_keeps_the_one_it_has() {
    // The prompt hook knows only an id, and it runs on every prompt — so a
    // nameless touch that blanked the name would leave a list of uuids.
    let pool = common::fresh_db().await;
    sessions::touch(&pool, "sess-1", Some("memview"))
        .await
        .expect("naming");
    sessions::touch(&pool, "sess-1", None)
        .await
        .expect("touching");
    sessions::touch(&pool, "sess-1", Some("   "))
        .await
        .expect("touching with a blank");

    let listed = sessions::list(&pool).await.expect("listing");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name.as_deref(), Some("memview"));
}

#[tokio::test]
async fn a_session_row_carries_how_much_it_is_holding() {
    let pool = common::fresh_db().await;
    sessions::touch(&pool, "sess-1", Some("memview"))
        .await
        .expect("recording");
    for n in 0..3 {
        repo::create(
            &pool,
            NewTask {
                repo: Some("memview".into()),
                subject: format!("Task {n}"),
                body: String::new(),
                assignee: Some(to_session("sess-1")),
            },
            &pippijn(),
        )
        .await
        .expect("filing");
    }
    let finished = repo::create(
        &pool,
        NewTask {
            repo: Some("memview".into()),
            subject: "Already done".into(),
            body: String::new(),
            assignee: Some(to_session("sess-1")),
        },
        &pippijn(),
    )
    .await
    .expect("filing");
    repo::update(
        &pool,
        finished.id,
        Change {
            status: Some(Status::Done),
            ..Default::default()
        },
        &pippijn(),
    )
    .await
    .expect("finishing");

    let listed = sessions::list(&pool).await.expect("listing");
    assert_eq!(listed[0].open, 3, "the finished one was counted as work");
}

#[tokio::test]
async fn a_repo_filter_never_returns_the_tasks_that_belong_to_no_repo() {
    // A session asks by repo, so this is what keeps Pippijn's own items — which
    // have no checkout — out of every prompt.
    let pool = common::fresh_db().await;
    repo::create(&pool, filed("memview", "In a repo"), &pippijn())
        .await
        .expect("filing");
    repo::create(
        &pool,
        NewTask {
            repo: None,
            subject: "Personal, no repo".into(),
            body: String::new(),
            assignee: Some(to_person()),
        },
        &pippijn(),
    )
    .await
    .expect("filing");

    let asked = repo::list(&pool, &Filter::open_in(vec!["memview".into()]))
        .await
        .expect("listing");
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].subject, "In a repo");

    let everything = repo::list(&pool, &Filter::default())
        .await
        .expect("listing");
    assert_eq!(everything.len(), 2);
}
