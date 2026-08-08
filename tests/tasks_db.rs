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
            include_closed: true,
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

#[tokio::test]
async fn finishing_a_task_records_who_finished_it() {
    // `assignee` is the only place a LIST can say who did something — the
    // history knows, and no list renders a history. A task closed while held by
    // nobody therefore read as "done by nobody" everywhere it was seen again.
    let pool = common::fresh_db().await;
    // The route touches the session before every write; a session row has to
    // exist for a task to point at one.
    sessions::touch(&pool, "sess-1", Some("tasks"))
        .await
        .expect("recording a session");
    let task = repo::create(&pool, filed("tasks", "Nobody is holding this"), &pippijn())
        .await
        .expect("filing");
    assert_eq!(task.assignee.kind, AssigneeKind::Nobody);

    let done = repo::update(
        &pool,
        task.id,
        Change {
            status: Some(Status::Done),
            ..Default::default()
        },
        &Actor::Session("sess-1".into()),
    )
    .await
    .expect("finishing");

    assert_eq!(done.assignee.kind, AssigneeKind::Session);
    assert_eq!(done.assignee.id.as_deref(), Some("sess-1"));
}

#[tokio::test]
async fn saying_where_a_finished_task_goes_beats_inferring_it() {
    // A caller naming an assignee is more specific than the rule above reading
    // one off the credential — handing work back while closing it must not be
    // silently rewritten into keeping it.
    let pool = common::fresh_db().await;
    let task = repo::create(&pool, filed("tasks", "Yours now"), &pippijn())
        .await
        .expect("filing");

    let done = repo::update(
        &pool,
        task.id,
        Change {
            status: Some(Status::Done),
            assignee: Some(to_person()),
            ..Default::default()
        },
        &Actor::Session("sess-1".into()),
    )
    .await
    .expect("finishing");

    assert_eq!(done.assignee.kind, AssigneeKind::Person);
    assert_eq!(done.assignee.id.as_deref(), Some("pippijn"));
}

#[tokio::test]
async fn reopening_a_task_leaves_its_holder_alone() {
    let pool = common::fresh_db().await;
    // The route touches the session before every write; a session row has to
    // exist for a task to point at one.
    sessions::touch(&pool, "sess-1", Some("tasks"))
        .await
        .expect("recording a session");
    let task = repo::create(&pool, filed("tasks", "Not finished after all"), &pippijn())
        .await
        .expect("filing");
    repo::update(
        &pool,
        task.id,
        Change {
            status: Some(Status::Done),
            ..Default::default()
        },
        &Actor::Session("sess-1".into()),
    )
    .await
    .expect("finishing");

    let reopened = repo::update(
        &pool,
        task.id,
        Change {
            status: Some(Status::Open),
            ..Default::default()
        },
        &pippijn(),
    )
    .await
    .expect("reopening");

    // Whoever last had it is a better guess than nobody, and reopening is not
    // a claim on the work.
    assert_eq!(reopened.assignee.id.as_deref(), Some("sess-1"));
}

#[tokio::test]
async fn a_migrated_task_is_reachable_by_what_it_used_to_be_called() {
    // 46% of the 598 imported tasks could not keep their number, because a
    // built-in number was unique only inside one session. `recall#79` is the
    // handle old prose still contains, so it has to resolve to something.
    let pool = common::fresh_db().await;
    let task = repo::create(
        &pool,
        filed("recall", "Came from somewhere else"),
        &pippijn(),
    )
    .await
    .expect("filing");
    sqlx::query("UPDATE tasks SET origin_session = 'recall', origin_number = 79 WHERE id = ?")
        .bind(task.id)
        .execute(&pool)
        .await
        .expect("recording where it came from");

    let found = repo::by_origin(&pool, "recall", 79)
        .await
        .expect("looking it up")
        .expect("a task");
    assert_eq!(found.task.id, task.id);
    assert_eq!(found.task.origin.as_deref(), Some("recall#79"));

    // A number that belonged to a different session is a different task, which
    // is the whole reason the pair is the key rather than the number.
    assert!(
        repo::by_origin(&pool, "health", 79)
            .await
            .expect("looking it up")
            .is_none()
    );
}

#[tokio::test]
async fn who_holds_what_counts_the_finished_work_too() {
    // `open` alone says who is busy and nothing about who has done anything: a
    // task leaves every open list the moment it is finished. `0/2` and `0/0`
    // are a session that has cleared its plate and one that never had one.
    let pool = common::fresh_db().await;
    sessions::touch(&pool, "sess-1", Some("recall"))
        .await
        .expect("recording a session");
    sessions::touch(&pool, "sess-2", Some("idle"))
        .await
        .expect("recording a session");

    for subject in ["One", "Two", "Three"] {
        let task = repo::create(&pool, filed("recall", subject), &pippijn())
            .await
            .expect("filing");
        repo::update(
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
        if subject != "Three" {
            repo::update(
                &pool,
                task.id,
                Change {
                    status: Some(Status::Done),
                    // Explicit, so the finisher rule does not move it away from
                    // the session whose tally this test is about.
                    assignee: Some(to_session("sess-1")),
                    ..Default::default()
                },
                &pippijn(),
            )
            .await
            .expect("finishing");
        }
    }
    // One for the person, one left in the pile.
    let mine = repo::create(&pool, filed("recall", "Mine"), &pippijn())
        .await
        .expect("filing");
    repo::update(
        &pool,
        mine.id,
        Change {
            assignee: Some(to_person()),
            ..Default::default()
        },
        &pippijn(),
    )
    .await
    .expect("taking it");
    repo::create(&pool, filed("recall", "Unclaimed"), &pippijn())
        .await
        .expect("filing");

    let holders = sessions::holders(&pool).await.expect("counting");
    let find = |kind: &str, id: Option<&str>| {
        holders
            .iter()
            .find(|h| h.kind == kind && h.id.as_deref() == id)
            .unwrap_or_else(|| panic!("no {kind} row for {id:?}"))
    };

    let busy = find("session", Some("sess-1"));
    assert_eq!((busy.open, busy.total), (1, 3));
    let idle = find("session", Some("sess-2"));
    assert_eq!((idle.open, idle.total), (0, 0));
    let person = find("person", Some("pippijn"));
    assert_eq!((person.open, person.total), (1, 1));
    let pile = find("nobody", None);
    assert_eq!((pile.open, pile.total), (1, 1));

    // Whoever is holding most comes first; the person and the pile are
    // landmarks rather than entries in that ranking, so they are always last.
    assert_eq!(holders[holders.len() - 2].kind, "person");
    assert_eq!(holders[holders.len() - 1].kind, "nobody");
}

/// The one test tying `Status::is_open` to the SQL that means the same thing.
///
/// ⚠ **This is the test the fourth status was added for.** Every query that
/// meant *open* said `status <> 'done'`, which was correct while there were
/// three states and became a silent miscount the moment `dropped` existed —
/// nothing would have failed, the numbers would just have been wrong. So this
/// puts one task in each of the four states and asks every counting query in
/// the service what it sees, rather than trusting that the six call sites were
/// all found.
#[tokio::test]
async fn a_dropped_task_is_not_open_anywhere() {
    let pool = common::fresh_db().await;
    let mut ids = Vec::new();
    for (subject, status) in [
        ("Still to do", Status::Open),
        ("In hand", Status::Doing),
        ("Finished", Status::Done),
        ("Overtaken by events", Status::Dropped),
    ] {
        let task = repo::create(&pool, filed("tasks", subject), &pippijn())
            .await
            .expect("filing");
        if status != Status::Open {
            repo::update(
                &pool,
                task.id,
                Change {
                    status: Some(status),
                    ..Default::default()
                },
                &pippijn(),
            )
            .await
            .expect("moving it along");
        }
        ids.push((task.id, status));
    }

    let open = repo::list(&pool, &Filter::default())
        .await
        .expect("listing");
    let open_subjects: Vec<&str> = open.iter().map(|t| t.subject.as_str()).collect();
    assert_eq!(
        open_subjects,
        ["Still to do", "In hand"],
        "the open list is exactly the tasks Status::is_open admits"
    );

    // The digest reads that same list, and this is the property the whole
    // service is built to keep: a dropped task never reaches a prompt.
    let digest = tasks::digest::render(&open);
    assert!(
        !digest.contains("Overtaken by events"),
        "a dropped task reached the digest: {digest}"
    );
    assert!(
        !digest.contains("Finished"),
        "a done task reached the digest"
    );

    let all = repo::list(
        &pool,
        &Filter {
            include_closed: true,
            ..Default::default()
        },
    )
    .await
    .expect("listing everything");
    assert_eq!(all.len(), 4, "closed means kept, both kinds of closed");

    // Closed is closed: a dropped task has a closing time, which `IF(? =
    // 'done', …)` would not have given it.
    let dropped = all
        .iter()
        .find(|t| t.status == Status::Dropped)
        .expect("the dropped task");
    assert!(
        dropped.closed_at.is_some(),
        "a dropped task was left with no closing time"
    );

    // The person filed and closed all four, so the finisher rule handed them
    // over — including the dropped one, which is the point: a list has to be
    // able to say who decided it was not worth doing.
    assert_eq!(dropped.assignee.kind, AssigneeKind::Person);

    // The filter bar's per-repo count, which is the query least like the others
    // and the one a reader is most likely to miss.
    let counts = tasks::routes::api::repos_with_work(&pool)
        .await
        .expect("counting repos");
    assert_eq!(counts.len(), 1);
    assert_eq!(counts[0].open, 2, "the filter bar counted a closed task");

    // Every session row carries its own open count, by a seventh query.
    sessions::touch(&pool, "sess-1", Some("tasks"))
        .await
        .expect("recording a session");
    let listed = sessions::list(&pool).await.expect("listing sessions");
    assert_eq!(listed[0].open, 0, "a session that holds nothing");

    // `open` is what is in hand; `total` is that plus what was done. The
    // dropped one is in neither, because it is not work and was not done.
    //
    // The person closed two of the four, so the finisher rule handed both to
    // them and left the other two in the pile. **`1` is the whole assertion**:
    // they closed two tasks and one of them counts, because the other was
    // dropped. Counting it would read as having finished twice the work.
    let holders = sessions::holders(&pool).await.expect("counting");
    let person = holders
        .iter()
        .find(|h| h.kind == "person")
        .expect("the person's row");
    assert_eq!(
        (person.open, person.total),
        (0, 1),
        "the dropped task was counted as work done"
    );
    let pile = holders
        .iter()
        .find(|h| h.kind == "nobody")
        .expect("the pile's row");
    assert_eq!(
        (pile.open, pile.total),
        (2, 2),
        "the pile is the two nobody has closed"
    );
}

/// Dropping and finishing are different answers to the same question, and the
/// difference has to survive being read back.
#[tokio::test]
async fn dropping_a_task_credits_nobody_with_doing_it() {
    let pool = common::fresh_db().await;
    sessions::touch(&pool, "sess-1", Some("memview"))
        .await
        .expect("recording a session");
    let task = repo::create(
        &pool,
        filed("memview", "Wait for a thing that never came"),
        &pippijn(),
    )
    .await
    .expect("filing");

    let dropped = repo::update(
        &pool,
        task.id,
        Change {
            status: Some(Status::Dropped),
            ..Default::default()
        },
        &Actor::Session("sess-1".into()),
    )
    .await
    .expect("dropping");
    assert_eq!(dropped.status, Status::Dropped);
    assert_eq!(dropped.assignee.id.as_deref(), Some("sess-1"));

    // The history says which of the two it was, in the words the status uses.
    let detail = repo::get(&pool, task.id)
        .await
        .expect("reading")
        .expect("the task");
    let moves: Vec<&str> = detail
        .events
        .iter()
        .filter(|e| e.kind == "status")
        .filter_map(|e| e.detail.as_deref())
        .collect();
    assert_eq!(moves, ["open → dropped"]);

    // Reopening it leaves the holder alone and takes the closing time back off.
    let reopened = repo::update(
        &pool,
        task.id,
        Change {
            status: Some(Status::Open),
            ..Default::default()
        },
        &pippijn(),
    )
    .await
    .expect("reopening");
    assert_eq!(reopened.assignee.id.as_deref(), Some("sess-1"));
    assert!(
        reopened.closed_at.is_none(),
        "a reopened task is still closed"
    );
}
