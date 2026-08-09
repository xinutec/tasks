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

/// Filed straight into the pile, which since 2026-08-09 has to be asked for:
/// leaving the assignee out means the task belongs to whoever filed it.
fn unclaimed(repo_name: &str, subject: &str) -> NewTask {
    NewTask {
        assignee: Some(Assignee::nobody()),
        ..filed(repo_name, subject)
    }
}

/// Every event kind on a task, oldest first.
async fn kinds(pool: &sqlx::MySqlPool, id: u64) -> Vec<String> {
    repo::get(pool, id)
        .await
        .expect("reading")
        .expect("a task")
        .events
        .iter()
        .map(|e| e.kind.clone())
        .collect()
}

#[tokio::test]
async fn a_filed_task_comes_back_open_and_held_by_whoever_filed_it() {
    let pool = common::fresh_db().await;
    let task = repo::create(&pool, filed("memview", "Something to do"), &pippijn())
        .await
        .expect("filing");
    assert_eq!(task.subject, "Something to do");
    assert_eq!(task.status, Status::Open);
    // Filing takes it on. The default was the pile until 2026-08-09, which meant
    // nothing was ever implicitly its filer's and a session's row could not say
    // what it was carrying.
    assert_eq!(task.assignee.kind, AssigneeKind::Person);
    assert_eq!(task.assignee.id.as_deref(), Some("pippijn"));
    assert!(!task.detailed, "no body was given");

    let listed = repo::list(&pool, &Filter::default())
        .await
        .expect("listing");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, task.id);
}

#[tokio::test]
async fn the_pile_is_something_said_rather_than_something_fallen_into() {
    let pool = common::fresh_db().await;
    let task = repo::create(
        &pool,
        NewTask {
            assignee: Some(Assignee::nobody()),
            ..filed("memview", "For whoever picks it up")
        },
        &Actor::Session("sess-1".into()),
    )
    .await
    .expect("filing");
    assert_eq!(task.assignee.kind, AssigneeKind::Nobody);

    // The history has to say it went to the pile, because that is now a
    // decision. `create` only records an `assigned` event for a holder that is
    // somebody, so this is the assertion that the silent case stays silent
    // while the row itself is right.
    let moves = kinds(&pool, task.id).await;
    assert_eq!(moves, vec!["created"], "filing to the pile moves nobody");
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
    // From the pile, so the two handovers under test are the whole history.
    let task = repo::create(&pool, unclaimed("memview", "Hand this over"), &pippijn())
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
    //
    // Touched first, and that is load-bearing since filing takes the task on:
    // the default holder is a foreign key into `sessions`, so a write by a
    // session the table has never heard of fails rather than quietly landing in
    // the pile. Both write routes touch before they reach the repo; a caller
    // coming in underneath them has to do the same.
    sessions::touch(&pool, "sess-9", None)
        .await
        .expect("recording a session");
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
    // Filed into the pile so that the object restated below is the object that
    // is there — `assignee: nobody` against a task the filer now holds would be
    // a real change, and the point of this test is that nothing changes.
    let task = repo::create(&pool, unclaimed("memview", "Steady"), &pippijn())
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
    let task = repo::create(
        &pool,
        unclaimed("tasks", "Nobody is holding this"),
        &pippijn(),
    )
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
    repo::create(&pool, unclaimed("recall", "Unclaimed"), &pippijn())
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
        // Into the pile, so that what each holder ends up with is the doing of
        // the status rules rather than of the filing.
        let task = repo::create(&pool, unclaimed("tasks", subject), &pippijn())
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
    // Of the four filed into the pile the person took three — one by starting
    // it and two by closing them — and left the one nobody has touched. So
    // `total` is **2**, and that is the whole assertion: they are holding three
    // tasks and only two of them count, because the third was dropped.
    // Counting it would read as half again as much work done.
    let holders = sessions::holders(&pool).await.expect("counting");
    let person = holders
        .iter()
        .find(|h| h.kind == "person")
        .expect("the person's row");
    assert_eq!(
        (person.open, person.total),
        (1, 2),
        "the dropped task was counted as work done"
    );
    let pile = holders
        .iter()
        .find(|h| h.kind == "nobody")
        .expect("the pile's row");
    assert_eq!(
        (pile.open, pile.total),
        (1, 1),
        "the pile is the one nobody has picked up"
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

/// The holder column has to be able to describe the present, not only the past.
///
/// ⚠ **This is the test the starter rule was added for.** A holder was recorded
/// when a task was CLOSED and at no other moment, so a session could show three
/// finished tasks and `0 open` while it was in the middle of a fourth — every
/// conversation looked idle for as long as it was actually working. `start` was
/// already documented as how a session takes a task on, and it moved nobody.
#[tokio::test]
async fn starting_a_task_claims_it_the_way_finishing_one_does() {
    let pool = common::fresh_db().await;
    sessions::touch(&pool, "sess-1", Some("tasks"))
        .await
        .expect("recording a session");
    sessions::touch(&pool, "sess-2", Some("memview"))
        .await
        .expect("recording a session");

    let task = repo::create(&pool, unclaimed("tasks", "In the pile"), &pippijn())
        .await
        .expect("filing");
    assert_eq!(task.assignee.kind, AssigneeKind::Nobody);

    let started = repo::update(
        &pool,
        task.id,
        Change {
            status: Some(Status::Doing),
            ..Default::default()
        },
        &Actor::Session("sess-1".into()),
    )
    .await
    .expect("starting");
    assert_eq!(started.status, Status::Doing);
    assert_eq!(started.assignee.kind, AssigneeKind::Session);
    assert_eq!(started.assignee.id.as_deref(), Some("sess-1"));
    // Resolved through the join, so a list can print a name rather than a
    // 36-character id.
    assert_eq!(started.assignee.name.as_deref(), Some("tasks"));

    // It reads as a handover in the history, because it is one.
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
    assert_eq!(moves, vec!["nobody → tasks"]);

    // A second conversation running `start` on one already held takes nothing:
    // a holder is inferred only where there is none. Taking work off another
    // session is a handover, which is what `move` is for.
    let poached = repo::update(
        &pool,
        task.id,
        Change {
            status: Some(Status::Doing),
            ..Default::default()
        },
        &Actor::Session("sess-2".into()),
    )
    .await
    .expect("starting again");
    assert_eq!(
        poached.assignee.id.as_deref(),
        Some("sess-1"),
        "a second session quietly took a task already being worked on"
    );
    assert_eq!(
        kinds(&pool, task.id).await,
        vec!["created", "status", "assigned"],
        "starting a task twice wrote a second history"
    );

    // The session's own row is the thing this exists for: work in flight, not
    // only work finished.
    let listed = sessions::list(&pool).await.expect("listing sessions");
    let busy = listed
        .iter()
        .find(|s| s.id == "sess-1")
        .expect("the session's row");
    assert_eq!(busy.open, 1, "a session in the middle of a task reads idle");
}

/// What a session is shown, and what it is not.
///
/// ⚠ **The digest is the only thing that costs anything per turn**, so who it
/// selects for is a cost question before it is a courtesy one. Until 2026-08-09
/// it filtered on repository alone — inherited from one `TASKS.md` per repo
/// holding both parties' work — and every session paid, every turn, for tasks
/// another conversation was already carrying.
#[tokio::test]
async fn a_session_digest_carries_its_own_work_and_the_pile() {
    let pool = common::fresh_db().await;
    for (id, name) in [("sess-1", "tasks"), ("sess-2", "memview")] {
        sessions::touch(&pool, id, Some(name))
            .await
            .expect("recording a session");
    }

    let mut filed_ids = Vec::new();
    for (subject, holder) in [
        ("Mine to do", Some(to_session("sess-1"))),
        ("Another conversation has this", Some(to_session("sess-2"))),
        ("Pippijn is holding this", Some(to_person())),
        ("For whoever picks it up", Some(Assignee::nobody())),
    ] {
        let task = repo::create(
            &pool,
            NewTask {
                assignee: holder,
                ..filed("tasks", subject)
            },
            &pippijn(),
        )
        .await
        .expect("filing");
        filed_ids.push(task.id);
    }
    assert_eq!(filed_ids.len(), 4);

    let mine = repo::list(&pool, &Filter::digest_for("sess-1", vec!["tasks".into()]))
        .await
        .expect("listing");
    let subjects: Vec<&str> = mine.iter().map(|t| t.subject.as_str()).collect();
    assert_eq!(
        subjects,
        ["Mine to do", "For whoever picks it up"],
        "a session's digest is its own work and the pile, in that order of id"
    );

    // The pile is the handover channel and losing it would be the real cost of
    // this change: work Pippijn leaves for whoever is around would become
    // invisible to everybody at once.
    assert!(
        subjects.contains(&"For whoever picks it up"),
        "the pile fell out of the digest"
    );

    // A person reading a digest without naming a session still sees everything
    // — that path is `task digest`, for measuring the cost.
    let everything = repo::list(&pool, &Filter::open_in(vec!["tasks".into()]))
        .await
        .expect("listing");
    assert_eq!(everything.len(), 4);
}

/// Starting a task somebody else was given must not take it off them.
#[tokio::test]
async fn starting_a_task_assigned_to_another_session_takes_nothing() {
    let pool = common::fresh_db().await;
    for (id, name) in [("sess-1", "tasks"), ("sess-2", "memview")] {
        sessions::touch(&pool, id, Some(name))
            .await
            .expect("recording a session");
    }
    // Pippijn hands it to one conversation, which has not got to it yet.
    let task = repo::create(
        &pool,
        NewTask {
            assignee: Some(to_session("sess-1")),
            ..filed("tasks", "Given to sess-1")
        },
        &pippijn(),
    )
    .await
    .expect("filing");

    let started = repo::update(
        &pool,
        task.id,
        Change {
            status: Some(Status::Doing),
            ..Default::default()
        },
        &Actor::Session("sess-2".into()),
    )
    .await
    .expect("starting");
    assert_eq!(
        started.assignee.id.as_deref(),
        Some("sess-1"),
        "another session's unstarted task was taken by starting it"
    );
}
