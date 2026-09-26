//! The JSON API, plus the one endpoint that answers in plain text.

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::access::{Access, SeenAs, Viewer};
use crate::digest;
use crate::error::AppError;
use crate::sessions;
use crate::state::AppState;
use crate::tasks::checks;
use crate::tasks::commands;
use crate::tasks::focus;
use crate::tasks::repo::{self, Change, Filter, NewTask};
use crate::tasks::types::{Revision, Task, TaskDetail, Updated};
use crate::tasks::work;
use crate::wire::{RequiredKeys, Wire};

/// Every `/api` path that is not a route.
///
/// Answers as the API, not the page. Deliberately *before* the credential
/// check: a 401 for a typo sends the reader to look at their token.
pub async fn not_found() -> AppError {
    AppError::NotFound
}

/// The name to record, which is a name only when the caller is naming itself.
///
/// ⚠ **The digest is where these come apart.** A person may read a session's
/// digest with `?session=`, and their browser is not that conversation, so no
/// header of theirs may rename it.
fn own_name<'a>(viewer: &Viewer, called: &'a Option<String>) -> Option<&'a str> {
    match viewer {
        Viewer::Session(_) => called.as_deref(),
        Viewer::Owner(_) => None,
    }
}

/// Who the caller is, as `/api/me` answers. Mirrored by `Me` in `models.ts`.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Me {
    Person { id: String, name: String },
    Session { id: String },
}

/// Who the caller is, so the client can draw itself correctly.
pub async fn me(Access(viewer): Access) -> Json<Me> {
    Json(match viewer {
        Viewer::Owner(user) => Me::Person {
            id: user.user_id,
            name: user.display_name,
        },
        Viewer::Session(id) => Me::Session { id },
    })
}

#[derive(Deserialize)]
pub struct DigestQuery {
    /// The conversation asking. Optional — a person can read a digest too. When
    /// present the session is recorded as seen.
    session: Option<String>,
}

/// The index a prompt receives: one line per open task, and nothing else.
///
/// **`text/plain`**: its consumer is a `UserPromptSubmit` hook that prints it on
/// every prompt, and JSON would put a parser on that path.
pub async fn digest(
    Access(viewer): Access,
    SeenAs(called): SeenAs,
    State(app): State<AppState>,
    Query(q): Query<DigestQuery>,
) -> Result<impl IntoResponse, AppError> {
    // The credential wins over the query parameter: a session must not mark
    // another one as alive.
    let session = match &viewer {
        Viewer::Session(id) => Some(id.clone()),
        Viewer::Owner(_) => q.session.clone(),
    };
    if let Some(id) = &session {
        sessions::touch(&app.db, id, own_name(&viewer, &called)).await?;
    }
    // A person naming no session gets everything: there is no "own" to narrow
    // to, and that path is for checking the cost.
    let filter = match &session {
        Some(id) => Filter::digest_for(id),
        None => Filter::default(),
    };
    let tasks = repo::list(&app.db, &filter).await?;
    // A focus belongs to a conversation, so a person naming none has none.
    let focus = match &session {
        Some(id) => focus::current(&app.db, id).await?,
        None => None,
    };
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        digest::render(&tasks, focus.as_ref()),
    ))
}

/// What a session says it is working on, for how long.
#[derive(Deserialize)]
pub struct NewFocus {
    tasks: BTreeSet<u64>,
    /// How long, in minutes. The wire carries the quantity; the spelling (`4h`)
    /// is the CLI's, read by [`focus::parse`].
    minutes: i64,
}

/// Enter a focus period.
///
/// ⚠ **A session may only focus itself.** A focus claims what one conversation
/// is doing, and letting one session quiet another's prompt would be the
/// worst-shaped feature in the service. A browser is not a conversation, so the
/// person has no focus either.
pub async fn start_focus(
    Access(viewer): Access,
    State(app): State<AppState>,
    Json(new): Json<NewFocus>,
) -> Result<Json<focus::Focus>, AppError> {
    let session = own_session(&viewer)?;
    let focus = focus::enter(
        &app.db,
        &session,
        &new.tasks,
        chrono::Duration::minutes(new.minutes),
    )
    .await?;
    Ok(Json(focus))
}

/// What `DELETE /api/focus` answers: the focus it ended, if there was one.
#[derive(Serialize)]
pub struct Ended {
    was: Option<focus::Focus>,
}

pub async fn end_focus(
    Access(viewer): Access,
    State(app): State<AppState>,
) -> Result<Json<Ended>, AppError> {
    let session = own_session(&viewer)?;
    let was = focus::current(&app.db, &session).await?;
    focus::leave(&app.db, &session).await?;
    Ok(Json(Ended { was }))
}

pub async fn read_focus(
    Access(viewer): Access,
    State(app): State<AppState>,
) -> Result<Json<Option<focus::Focus>>, AppError> {
    let session = own_session(&viewer)?;
    Ok(Json(focus::current(&app.db, &session).await?))
}

/// The conversation making the request, or a refusal naming why there is none.
fn own_session(viewer: &Viewer) -> Result<String, AppError> {
    match viewer {
        Viewer::Session(id) => Ok(id.clone()),
        Viewer::Owner(_) => Err(AppError::BadRequest(
            "a focus belongs to a conversation, and this request is a person's. \
             There is nothing to narrow a browser's reading to."
                .into(),
        )),
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    /// Include closed tasks — done and dropped alike. Spelled `done` on the wire
    /// because that is what callers send.
    #[serde(default)]
    done: bool,
    session: Option<String>,
    person: Option<String>,
    /// Widen `session` to *and the ones nobody holds*.
    ///
    /// "What am I holding" (`--mine`) and "what could I pick up" (bare `task
    /// list`, the digest's rule) are both asked. Ignored without `session`, as
    /// [`Filter::or_unheld`] is.
    #[serde(default)]
    pile: bool,
    /// Strictly the tasks nobody holds. Wins over `session` and `person`.
    ///
    /// The narrow twin of `pile`, which *widens*; the CLI's `--pile` is this one.
    #[serde(default)]
    unheld: bool,
    /// Tasks this session filed and does not hold — see
    /// [`Filter::handed_out_by`].
    ///
    /// A session id, not a flag: the caller need not be its subject.
    handed_out: Option<String>,
}

pub async fn list(
    Access(_): Access,
    State(app): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<Task>>, AppError> {
    let filter = Filter {
        include_closed: q.done,
        session: q.session,
        person: q.person,
        or_unheld: q.pile,
        unheld: q.unheld,
        handed_out_by: q.handed_out,
    };
    Ok(Json(repo::list(&app.db, &filter).await?))
}

pub async fn detail(
    Access(_): Access,
    State(app): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<TaskDetail>, AppError> {
    repo::get(&app.db, id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

/// The task as it stood before its most recent edit.
///
/// ⚠ **A separate path, not a field on `detail`**, which every reader opens: a
/// second whole body there would serve only the rare undo.
///
/// 404 when nothing has overwritten this task — to a caller the same as no
/// task: nothing to put back.
pub async fn previous(
    Access(who): Access,
    State(app): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<Revision>, AppError> {
    repo::previous(&app.db, id, &who.actor())
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

/// Who holds what — every session, Pippijn, and the pile.
pub async fn holders(
    Access(_): Access,
    State(app): State<AppState>,
) -> Result<Json<Vec<sessions::Holder>>, AppError> {
    Ok(Json(sessions::holders(&app.db).await?))
}

pub async fn create(
    Access(viewer): Access,
    SeenAs(called): SeenAs,
    State(app): State<AppState>,
    Wire(new): Wire<NewTask>,
) -> Result<Json<Task>, AppError> {
    let actor = viewer.actor();
    // ⚠ **Before the write and before `touch`.** Skipping the duplicate check is
    // licensed only by a recent refusal of the same subject; without one
    // nothing lands, not even the session's `last_seen`. Sessions otherwise
    // pass the skip flag pre-emptively, and the check never runs.
    if !new.checked {
        let Viewer::Session(id) = &viewer else {
            return Err(AppError::BadRequest(
                "only a conversation can skip the duplicate check, and this request is a \
                 person's."
                    .into(),
            ));
        };
        if !checks::refused_recently(&app.db, id, &new.subject).await? {
            return Err(AppError::BadRequest(checks::unlicensed()));
        }
    }
    if let Viewer::Session(id) = &viewer {
        sessions::touch(&app.db, id, called.as_deref()).await?;
    }
    Ok(Json(repo::create(&app.db, new, &actor).await?))
}

/// Change a task — its status, its holder, its words. Partial: an absent field
/// is left alone.
pub async fn update(
    Access(viewer): Access,
    SeenAs(called): SeenAs,
    State(app): State<AppState>,
    Path(id): Path<u64>,
    Wire(change): Wire<Change>,
) -> Result<Json<Updated>, AppError> {
    let actor = viewer.actor();
    if let Viewer::Session(session) = &viewer {
        sessions::touch(&app.db, session, called.as_deref()).await?;
    }
    Ok(Json(repo::update(&app.db, id, change, &actor).await?))
}

/// Every session known, with how much each is holding.
pub async fn session_list(
    Access(_): Access,
    State(app): State<AppState>,
) -> Result<Json<Vec<sessions::Session>>, AppError> {
    Ok(Json(sessions::list(&app.db).await?))
}

#[derive(Deserialize)]
pub struct Rename {
    pub name: String,
}

/// `name` is not listed: the type refuses a rename without one anyway, and
/// listing a key only buys a sentence about an unguessable answer, which here
/// there is none.
impl RequiredKeys for Rename {}

/// Tell the service what a session now calls itself.
///
/// ⚠ **One column, and nothing moves**: the id is the identity. A session may
/// only rename itself — renaming another only makes a list unreadable.
pub async fn rename(
    Access(viewer): Access,
    State(app): State<AppState>,
    Path(id): Path<String>,
    Wire(body): Wire<Rename>,
) -> Result<impl IntoResponse, AppError> {
    if let Viewer::Session(own) = &viewer
        && own != &id
    {
        return Err(AppError::Forbidden);
    }
    // ⚠ **Blank is refused, not passed on.** `touch` reads an empty name as *no
    // name given* and keeps the old one, which would answer 204 to a write
    // that changed nothing.
    let name = body.name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest(
            "a session's name cannot be blank: the id is the identity and the name is what a \
             list calls it, so an empty one reads as a conversation called \"\". Leave it \
             unnamed, or give it a word"
                .into(),
        ));
    }
    sessions::touch(&app.db, &id, Some(name)).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Record what a model check did.
///
/// ⚠ **A conversation's report on its own tooling.** Checks are spawned by the
/// CLI around a write; a browser never runs one, so the owner is refused and
/// every row is a check that ran.
///
/// Clock and session come from the request, not the body, so a run cannot be
/// filed as somebody else's or backdated.
pub async fn check_ran(
    Access(viewer): Access,
    State(app): State<AppState>,
    Wire(run): Wire<checks::Run>,
) -> Result<axum::http::StatusCode, AppError> {
    let Viewer::Session(session) = &viewer else {
        return Err(AppError::BadRequest(
            "a check belongs to the conversation that ran it, and this request is a person's."
                .into(),
        ));
    };
    checks::record(&app.db, session, &run).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// How far back to read the checks.
#[derive(Deserialize)]
pub struct ChecksQuery {
    #[serde(default = "a_week")]
    days: u32,
}

fn a_week() -> u32 {
    7
}

/// What the checks have been doing, as rows.
///
/// **Rows, not the summary** `task checks` folds them into: questions like
/// where `PATIENCE` should sit are asked of a distribution.
pub async fn checks_ran(
    Access(_viewer): Access,
    State(app): State<AppState>,
    Query(window): Query<ChecksQuery>,
) -> Result<Json<Vec<checks::Ran>>, AppError> {
    Ok(Json(checks::recent(&app.db, window.days).await?))
}

/// Record one command the CLI ran.
///
/// ⚠ **A session's, like a check's**: a person in the web UI runs no CLI.
pub async fn command_ran(
    Access(viewer): Access,
    State(app): State<AppState>,
    Wire(run): Wire<commands::Run>,
) -> Result<Json<Carry>, AppError> {
    let Viewer::Session(session) = &viewer else {
        return Err(AppError::BadRequest(
            "a command belongs to the conversation that ran it, and this request is a person's."
                .into(),
        ));
    };
    commands::record(&app.db, session, &run).await?;
    // ⚠ **The answer to a write, not a second request**: this path must cost
    // the command nothing, and the service already knows what to answer.
    let due = commands::due_to_report(&app.db).await.unwrap_or(false);
    if !due {
        return Ok(Json(Carry { report: None }));
    }
    let window = commands::recent(&app.db, 1).await.unwrap_or_default();
    let checks = checks::recent(&app.db, 1).await.unwrap_or_default();
    // ⚠ **Absent, not zeroed, when the count fails**: zero open tasks is a real
    // reading, and an error must not publish "the backlog is clear".
    let work = work::standing(&app.db).await.ok();
    Ok(Json(Carry {
        report: Some(Report {
            interval_s: commands::REPORTING_INTERVAL_S,
            commands: commands::tally(&window),
            checks: checks::tally(&checks),
            work,
        }),
    }))
}

/// What a caller is handed back after recording a command.
///
/// ⚠ **`report` is absent almost every time.** One caller per reporting window
/// ([`commands::due_to_report`]) is told to carry the numbers out; every other
/// command gets an empty object.
#[derive(Serialize)]
pub struct Carry {
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<Report>,
}

/// The numbers to carry, and the cadence to declare with them.
#[derive(Serialize)]
pub struct Report {
    /// Passed through, not decided by the caller: how long silence is tolerated
    /// belongs to the measurement.
    interval_s: u64,
    commands: Vec<commands::Tally>,
    checks: Vec<checks::Tally>,
    /// What is standing in the tracker — the WORK, not the tool. Absent when the
    /// count could not be taken.
    #[serde(skip_serializing_if = "Option::is_none")]
    work: Option<work::Tally>,
}

/// What the CLI has been doing, newest first.
///
/// Rows, not a summary, for the reason [`checks_ran`] gives.
pub async fn commands_ran(
    Access(_viewer): Access,
    State(app): State<AppState>,
    Query(window): Query<ChecksQuery>,
) -> Result<Json<Vec<commands::Ran>>, AppError> {
    Ok(Json(commands::recent(&app.db, window.days).await?))
}
