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
use crate::tasks::lifecycle;
use crate::tasks::repo::{self, Change, Filter, NewTask};
use crate::tasks::types::{Created, Revision, Task, TaskDetail, Updated};
use crate::tasks::work;
use crate::wire::{RequiredKeys, Wire};

/// Every `/api` path that is not a route, answered as the API, and before the
/// credential check: a 401 for a typo sends the reader to check their token.
pub async fn not_found() -> AppError {
    AppError::NotFound
}

/// The name to record, only when the caller names itself: a person reading a
/// session's digest with `?session=` must not rename it.
fn own_name<'a>(viewer: &Viewer, called: &'a Option<String>) -> Option<&'a str> {
    match viewer {
        Viewer::Session(_) => called.as_deref(),
        Viewer::Owner(_) => None,
    }
}

/// As `/api/me` answers; mirrored by `Me` in `models.ts`.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Me {
    Person { id: String, name: String },
    Session { id: String },
}

/// Who the caller is, so the client can draw itself.
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
    /// The conversation asking, recorded as seen. Optional: a person can read a digest.
    session: Option<String>,
}

/// The index a prompt receives. Plain text: the hook prints it every prompt,
/// and JSON would put a parser on that path.
pub async fn digest(
    Access(viewer): Access,
    SeenAs(called): SeenAs,
    State(app): State<AppState>,
    Query(q): Query<DigestQuery>,
) -> Result<impl IntoResponse, AppError> {
    // The credential wins: a session must not mark another as alive.
    let session = match &viewer {
        Viewer::Session(id) => Some(id.clone()),
        Viewer::Owner(_) => q.session.clone(),
    };
    if let Some(id) = &session {
        sessions::touch(&app.db, id, own_name(&viewer, &called)).await?;
    }
    // A person naming no session gets everything, to check the cost.
    let filter = match &session {
        Some(id) => Filter::digest_for(id),
        None => Filter::default(),
    };
    let tasks = repo::list(&app.db, &filter).await?;
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
    /// Minutes; the CLI parses spellings like `4h` ([`focus::parse`]).
    minutes: i64,
}

/// Enter a focus period. Only for oneself: letting a session quiet another's
/// prompt would be the worst feature here, and a browser has no focus.
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
    /// Include closed tasks, done and dropped (`done` on the wire).
    #[serde(default)]
    done: bool,
    session: Option<String>,
    person: Option<String>,
    /// Widen `session` to include the pile, as bare `task list` does. Needs `session`.
    #[serde(default)]
    pile: bool,
    /// Strictly the pile; wins over `session` and `person` (`--pile`).
    #[serde(default)]
    unheld: bool,
    /// Tasks this session filed and doesn't hold ([`Filter::handed_out_by`]); a session id.
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

/// The task before its latest edit: its own path, so every `detail` doesn't
/// carry a second body. 404 when nothing was overwritten.
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

/// Who holds what: sessions, the person, the pile.
pub async fn holders(
    Access(_): Access,
    State(app): State<AppState>,
) -> Result<Json<Vec<sessions::Holder>>, AppError> {
    Ok(Json(sessions::holders(&app.db, app.cfg.person()).await?))
}

pub async fn create(
    Access(viewer): Access,
    SeenAs(called): SeenAs,
    State(app): State<AppState>,
    Wire(new): Wire<NewTask>,
) -> Result<Json<Created>, AppError> {
    let actor = viewer.actor();
    // Before the write and before `touch`: skipping the duplicate check needs a
    // recent refusal of this subject, overridden by naming every task it named.
    // Otherwise nothing lands.
    if !new.checked {
        let Viewer::Session(id) = &viewer else {
            return Err(AppError::BadRequest(
                "only a conversation can skip the duplicate check, and this request is a \
                 person's."
                    .into(),
            ));
        };
        let Some(named) = checks::licence(&app.db, id, &new.subject).await? else {
            return Err(AppError::BadRequest(checks::unlicensed()));
        };
        let missing: Vec<u64> = named
            .into_iter()
            .filter(|id| !new.not_duplicate_of.contains(id))
            .collect();
        if !missing.is_empty() {
            return Err(AppError::BadRequest(checks::unnamed(&missing)));
        }
    }
    if let Viewer::Session(id) = &viewer {
        sessions::touch(&app.db, id, called.as_deref()).await?;
    }
    let named = lifecycle::mentioned(&format!("{}\n{}", new.subject, new.body));
    let task = repo::create(&app.db, new, &actor).await?;
    // The task is written; a failed hint must not send the caller to file it again.
    let closed = lifecycle::closed_by(&app.db, &actor, &named)
        .await
        .unwrap_or_else(|why| {
            tracing::warn!("looking for recently closed tasks a filing names: {why}");
            Vec::new()
        });
    Ok(Json(Created { task, closed }))
}

/// Change a task; an absent field is left alone.
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

/// Every session, with how much each holds.
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

/// No required keys listed: the type refuses a rename without a name anyway.
impl RequiredKeys for Rename {}

/// A session renames itself (only itself); the id is the identity, so nothing moves.
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
    // Blank is refused: `touch` would keep the old name and answer 204 for nothing.
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

/// Record what a model check did. A conversation's own report, so a browser is
/// refused; session and clock come from the request, never the body.
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

#[derive(Deserialize)]
pub struct LicenceQuery {
    subject: String,
}

/// What this conversation's latest refusal of a subject named, so the CLI skips the check only when the service will accept it.
pub async fn check_licence(
    Access(viewer): Access,
    State(app): State<AppState>,
    Query(q): Query<LicenceQuery>,
) -> Result<Json<checks::Licence>, AppError> {
    let Viewer::Session(session) = &viewer else {
        return Err(AppError::BadRequest(
            "only a conversation is refused a filing, and this request is a person's.".into(),
        ));
    };
    Ok(Json(checks::Licence {
        matched: checks::licence(&app.db, session, &q.subject).await?,
    }))
}

/// Per conversation: refusals, and filings past one.
pub async fn check_overrules(
    Access(_viewer): Access,
    State(app): State<AppState>,
    Query(window): Query<ChecksQuery>,
) -> Result<Json<Vec<checks::Overrules>>, AppError> {
    Ok(Json(checks::overrules(&app.db, window.days).await?))
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

/// The checks as rows, not `task checks`' summary: tuning `PATIENCE` needs the distribution.
pub async fn checks_ran(
    Access(_viewer): Access,
    State(app): State<AppState>,
    Query(window): Query<ChecksQuery>,
) -> Result<Json<Vec<checks::Ran>>, AppError> {
    Ok(Json(checks::recent(&app.db, window.days).await?))
}

/// Record one command the CLI ran; a session's only.
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
    // Handed back with this write, so reporting costs no extra request.
    let due = commands::due_to_report(&app.db).await.unwrap_or(false);
    if !due {
        return Ok(Json(Carry { report: None }));
    }
    let window = commands::recent(&app.db, 1).await.unwrap_or_default();
    let checks = checks::recent(&app.db, 1).await.unwrap_or_default();
    // Absent, not zero, when the count fails: zero is a real reading.
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

/// Usually empty: one caller per window carries the report ([`commands::due_to_report`]).
#[derive(Serialize)]
pub struct Carry {
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<Report>,
}

/// The numbers to carry, and the cadence to declare with them.
#[derive(Serialize)]
pub struct Report {
    /// How long silence is tolerated belongs to the measurement, not the caller.
    interval_s: u64,
    commands: Vec<commands::Tally>,
    checks: Vec<checks::Tally>,
    /// The backlog, not the tool. Absent when it couldn't be counted.
    #[serde(skip_serializing_if = "Option::is_none")]
    work: Option<work::Tally>,
}

/// What the CLI has been doing, newest first, as rows (see [`checks_ran`]).
pub async fn commands_ran(
    Access(_viewer): Access,
    State(app): State<AppState>,
    Query(window): Query<ChecksQuery>,
) -> Result<Json<Vec<commands::Ran>>, AppError> {
    Ok(Json(commands::recent(&app.db, window.days).await?))
}
