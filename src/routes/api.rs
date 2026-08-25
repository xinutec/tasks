//! The JSON API, plus the one endpoint that answers in plain text.

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::json;

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
use crate::wire::{RequiredKeys, Wire};

/// Every `/api` path that is not a route.
///
/// Answers as the API rather than as the app: a caller here wants JSON and an
/// error it can read, not the page. Deliberately *before* the credential check —
/// there is nothing behind a path that does not exist, and a 401 for a typo
/// sends the reader to look at their token.
pub async fn not_found() -> AppError {
    AppError::NotFound
}

/// The name to record, which is a name only when the caller is naming itself.
///
/// ⚠ **The digest is the one place these can come apart.** A person may read a
/// session's digest by passing `?session=`, and their browser is not that
/// conversation — so the header they are not sending must not become that
/// session's name, and a header they *are* sending must not either. Same
/// reasoning as the line above it: a caller cannot mark another session as
/// alive, and it cannot rename one here either.
fn own_name<'a>(viewer: &Viewer, called: &'a Option<String>) -> Option<&'a str> {
    match viewer {
        Viewer::Session(_) => called.as_deref(),
        Viewer::Owner(_) => None,
    }
}

/// Who the caller is, so the client can draw itself correctly.
pub async fn me(Access(viewer): Access) -> Json<serde_json::Value> {
    Json(match viewer {
        Viewer::Owner(user) => json!({
            "kind": "person",
            "id": user.user_id,
            "name": user.display_name,
        }),
        Viewer::Session(id) => json!({ "kind": "session", "id": id }),
    })
}

#[derive(Deserialize)]
pub struct DigestQuery {
    /// The conversation asking. Optional — a person can read a digest too — and
    /// when present the session is recorded as seen, which is the only way a
    /// row for it ever comes to exist.
    session: Option<String>,
}

/// The index a prompt receives: one line per open task, and nothing else.
///
/// **`text/plain`, and that is not laziness.** Its consumer is a
/// `UserPromptSubmit` hook whose entire contract is to print this and print
/// nothing else, on every prompt, on the machine whose message latency is
/// already the complaint. Handing it JSON would put a parser on that path to
/// produce the same eight lines.
pub async fn digest(
    Access(viewer): Access,
    SeenAs(called): SeenAs,
    State(app): State<AppState>,
    Query(q): Query<DigestQuery>,
) -> Result<impl IntoResponse, AppError> {
    // Whoever the credential says is asking wins over the query parameter — a
    // session must not be able to mark another one as alive.
    let session = match &viewer {
        Viewer::Session(id) => Some(id.clone()),
        Viewer::Owner(_) => q.session.clone(),
    };
    if let Some(id) = &session {
        sessions::touch(&app.db, id, own_name(&viewer, &called)).await?;
    }
    // What a session is shown: its own open tasks and the pile — not the ones
    // another conversation is holding.
    //
    // ⚠ **This used to also narrow by the repositories the session had
    // claimed, and that half was inherited from the storage rather than
    // chosen.** One `TASKS.md` per repository meant both parties' work sat in
    // one file, so seeing across holders was a side effect of there being
    // nowhere else to put it. Worse, it made the empty digest ambiguous: a
    // session that had claimed nothing saw exactly what a broken service looks
    // like. Dropped in `0004`, and the pile is global now — 3 unheld of 134
    // open when that was measured, which is what makes it affordable.
    //
    // A person reading a digest without naming a session gets everything: that
    // path is `task digest` for checking the cost, and there is no "own" to
    // narrow to.
    let filter = match &session {
        Some(id) => Filter::digest_for(id),
        None => Filter::default(),
    };
    let tasks = repo::list(&app.db, &filter).await?;
    // What this session said it was working on, if it said so and the hour has
    // not passed. A person reading a digest without naming a session has no
    // focus to apply — there is no conversation whose afternoon it is.
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
    /// How long, in minutes. A number rather than `4h`: the spelling is the
    /// CLI's business and [`focus::parse`] is where it is read, so the wire
    /// carries the quantity and one side does the reading.
    minutes: i64,
}

/// Enter a focus period.
///
/// ⚠ **A session may only focus itself.** A focus is a claim about what one
/// conversation is doing this afternoon, so there is nobody else who could make
/// it — and a route that let one session quiet another's prompt would be the
/// worst-shaped feature in the service. The person reading the app has no focus
/// for the same reason: a browser is not a conversation.
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

/// End one early, or report that there was not one.
pub async fn end_focus(
    Access(viewer): Access,
    State(app): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let session = own_session(&viewer)?;
    let was = focus::current(&app.db, &session).await?;
    focus::leave(&app.db, &session).await?;
    Ok(Json(json!({ "was": was })))
}

/// What this session is focused on, if anything.
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
    /// Include closed tasks — the done and the dropped alike. Off unless asked,
    /// everywhere. Still spelled `done` on the wire: it is what every existing
    /// caller sends, and "show me the closed ones too" is what it always meant.
    #[serde(default)]
    done: bool,
    session: Option<String>,
    person: Option<String>,
    /// Widen `session` to *and the ones nobody holds*.
    ///
    /// Asked for rather than assumed, because the two questions are different
    /// and both are wanted: "what am I holding" is a plate, and "what could I
    /// pick up" is a plate plus the pile. `--mine` is the first; the CLI's bare
    /// `task list` is the second, which is the digest's own rule and the reason
    /// this parameter exists at all. Ignored without `session`, exactly as
    /// [`Filter::or_unheld`] is — on its own it would mean every task there is.
    #[serde(default)]
    pile: bool,
    /// Strictly the tasks nobody holds. Wins over `session` and `person`.
    ///
    /// The narrow twin of `pile`, which *widens*. Both names are on the wire
    /// because both questions are asked, and telling them apart in one word is
    /// what the CLI's `--pile` spends its own flag on.
    #[serde(default)]
    unheld: bool,
}

/// Tasks matching a filter.
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
    };
    Ok(Json(repo::list(&app.db, &filter).await?))
}

/// One task, with its prose and its history.
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
/// ⚠ **A separate path rather than a field on `detail`.** A previous version is
/// a second whole body, and `GET /api/tasks/{id}` is what the app opens a task
/// with — putting it there would double that payload for every reader to serve
/// the rare one who is undoing something. Asked for by id, it costs nothing
/// until it is wanted.
///
/// 404 when nothing has overwritten this task, which is the same answer as a
/// task that does not exist and means the same thing to a caller: there is
/// nothing here to put back.
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

/// File a task.
pub async fn create(
    Access(viewer): Access,
    SeenAs(called): SeenAs,
    State(app): State<AppState>,
    Wire(new): Wire<NewTask>,
) -> Result<Json<Task>, AppError> {
    let actor = viewer.actor();
    // ⚠ **Before the write and before `touch`.** A filing that skipped the check
    // without having been refused is not filed at all, so nothing about it may
    // land first — and a session must not be marked alive by a request that is
    // about to be turned away.
    //
    // Measured over every transcript: 63 of 644 filings passed the flag on the
    // way in and only 16 followed a refusal. For the other 47 the check never
    // ran, so the trade this whole module rests on never happened.
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

/// `name` is not listed, and that is the proportionate answer rather than an
/// omission. The type refuses a rename that names nothing either way; what
/// listing a key buys is a sentence explaining an answer a caller would not
/// guess, and here there is only one thing to send. Compare `NewTask`, where
/// the unguessable answer — `null`, for *nobody has judged this* — is the whole
/// reason the key is required at all.
impl RequiredKeys for Rename {}

/// Tell the service what a session now calls itself.
///
/// ⚠ **A rename is an UPDATE of one column and moves nothing.** The id is the
/// identity; this is why. A session may only rename itself — the id in the path
/// has to be the one it authenticated as — because a session renaming another
/// is a way to make a list unreadable and there is no reason to want it.
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
    // ⚠ **Blank is refused rather than passed on.** `touch` reads an empty name
    // as *no name given* — it trims and filters, and its `COALESCE(VALUES(name),
    // name)` then keeps whatever was there. That is correct for a touch, which
    // runs on every request and must never wipe a name; here it made the route
    // answer 204 to a write that changed nothing. Somebody clearing the field
    // means to clear it, and has to be told that is not on offer.
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
/// ⚠ **A conversation's report about its own tooling, so a browser has nothing
/// to say here.** The person reading the app never runs a check: the two that
/// exist are spawned by the CLI, on the caller's machine, either side of a
/// write. Refusing the owner keeps the table what it claims to be — every row a
/// check that actually ran.
///
/// The clock and the session are taken from the request rather than from the
/// body, because a caller that could name either could file a run as somebody
/// else's or date it to a week ago, and both would be invisible in the numbers
/// the table exists to produce.
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
/// **Rows rather than the summary.** `task checks` folds them into two lines,
/// and the questions that made this table — what the density read fires on,
/// where `PATIENCE` should sit — are asked of a distribution. A route that
/// answered only in averages would have to be replaced by the first person who
/// wanted a percentile.
pub async fn checks_ran(
    Access(_viewer): Access,
    State(app): State<AppState>,
    Query(window): Query<ChecksQuery>,
) -> Result<Json<Vec<checks::Ran>>, AppError> {
    Ok(Json(checks::recent(&app.db, window.days).await?))
}

/// Record one command the CLI ran.
///
/// ⚠ **A session's, like a check's.** The holder of a command is the
/// conversation that typed it, and a person browsing the web UI is not running
/// the CLI — so there is no arm here for a cookie.
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
    // ⚠ **The answer to a write, not a second request.** The caller is already
    // here and the service already knows both things it needs — whether anybody
    // has reported lately, and what the numbers are. Making it ask separately
    // would put two more round trips on a path whose whole discipline is
    // costing the command nothing.
    let due = commands::due_to_report(&app.db).await.unwrap_or(false);
    if !due {
        return Ok(Json(Carry { report: None }));
    }
    let window = commands::recent(&app.db, 1).await.unwrap_or_default();
    let checks = checks::recent(&app.db, 1).await.unwrap_or_default();
    Ok(Json(Carry {
        report: Some(Report {
            interval_s: commands::REPORTING_INTERVAL_S,
            commands: commands::tally(&window),
            checks: checks::tally(&checks),
        }),
    }))
}

/// What a caller is handed back after recording a command.
///
/// ⚠ **`report` is absent almost every time, and that is the shape.** One
/// caller an hour is told to carry the numbers out; every other command gets an
/// empty object and does nothing. An arm that always carried the tally would put
/// a day of rows on the wire for every `task list` anybody runs.
#[derive(Serialize)]
pub struct Carry {
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<Report>,
}

/// The numbers to carry, and the cadence to declare with them.
#[derive(Serialize)]
pub struct Report {
    /// Passed through rather than decided by the caller: how long silence is
    /// tolerated is a property of the measurement, not of whichever session
    /// happened to run a command at the right moment.
    interval_s: u64,
    commands: Vec<commands::Tally>,
    checks: Vec<checks::Tally>,
}

/// What the CLI has been doing, newest first.
///
/// Rows rather than a summary, for the reason `checks_ran` gives: the caller
/// decides what question to ask of them, and a tally computed here would be the
/// only shape anybody could get.
pub async fn commands_ran(
    Access(_viewer): Access,
    State(app): State<AppState>,
    Query(window): Query<ChecksQuery>,
) -> Result<Json<Vec<commands::Ran>>, AppError> {
    Ok(Json(commands::recent(&app.db, window.days).await?))
}
