//! The JSON API, plus the one endpoint that answers in plain text.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::json;

use crate::access::{Access, Viewer};
use crate::digest;
use crate::error::AppError;
use crate::sessions;
use crate::state::AppState;
use crate::tasks::repo::{self, Change, Filter, NewTask};
use crate::tasks::types::{Task, TaskDetail, Updated};

/// Every `/api` path that is not a route.
///
/// Answers as the API rather than as the app: a caller here wants JSON and an
/// error it can read, not the page. Deliberately *before* the credential check —
/// there is nothing behind a path that does not exist, and a 401 for a typo
/// sends the reader to look at their token.
pub async fn not_found() -> AppError {
    AppError::NotFound
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
        sessions::touch(&app.db, id, None).await?;
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
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        digest::render(&tasks),
    ))
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
    State(app): State<AppState>,
    Json(new): Json<NewTask>,
) -> Result<Json<Task>, AppError> {
    let actor = viewer.actor();
    if let Viewer::Session(id) = &viewer {
        sessions::touch(&app.db, id, None).await?;
    }
    Ok(Json(repo::create(&app.db, new, &actor).await?))
}

/// Change a task — its status, its holder, its words. Partial: an absent field
/// is left alone.
pub async fn update(
    Access(viewer): Access,
    State(app): State<AppState>,
    Path(id): Path<u64>,
    Json(change): Json<Change>,
) -> Result<Json<Updated>, AppError> {
    let actor = viewer.actor();
    if let Viewer::Session(session) = &viewer {
        sessions::touch(&app.db, session, None).await?;
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
    Json(body): Json<Rename>,
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
