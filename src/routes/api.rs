//! The JSON API, plus the one endpoint that answers in plain text.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::access::{Access, Viewer};
use crate::digest;
use crate::error::AppError;
use crate::sessions;
use crate::state::AppState;
use crate::tasks::repo::{self, Change, Filter, NewTask};
use crate::tasks::types::{Task, TaskDetail};

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

/// A comma-separated repository list, as every query here takes it.
///
/// Repeated query keys are not what `serde_urlencoded` decodes into a `Vec`, and
/// the alternative — a hand-rolled parse of the raw query string — is a parser
/// on the path of every prompt. One separator, spelled out in the API doc.
fn repo_list(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[derive(Deserialize)]
pub struct DigestQuery {
    repos: Option<String>,
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
    let tasks = repo::list(&app.db, &Filter::open_in(repo_list(q.repos.as_deref()))).await?;
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
    repos: Option<String>,
    /// Include finished tasks. Off unless asked, everywhere.
    #[serde(default)]
    done: bool,
    session: Option<String>,
    person: Option<String>,
}

/// Tasks matching a filter.
pub async fn list(
    Access(_): Access,
    State(app): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<Task>>, AppError> {
    let filter = Filter {
        repos: repo_list(q.repos.as_deref()),
        include_done: q.done,
        session: q.session,
        person: q.person,
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

/// One task, found by what it was called before the migration — `/tasks/by/recall/79`.
///
/// **Two path segments rather than `/tasks/recall%2379`.** A `#` in a URL is the
/// fragment delimiter, so the natural spelling would have to be escaped by every
/// caller and would silently truncate to `/tasks/recall` for any that forgot.
pub async fn by_origin(
    Access(_): Access,
    State(app): State<AppState>,
    Path((session, number)): Path<(String, u64)>,
) -> Result<Json<TaskDetail>, AppError> {
    repo::by_origin(&app.db, &session, number)
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
) -> Result<Json<Task>, AppError> {
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
    sessions::touch(&app.db, &id, Some(&body.name)).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct RepoCount {
    /// `None` is the pile of tasks belonging to no checkout.
    pub repo: Option<String>,
    pub open: i64,
}

/// Which repositories have work, and how much — the client's filter bar.
pub async fn repos_with_work(app: &AppState) -> Result<Vec<RepoCount>, AppError> {
    let rows: Vec<(Option<String>, i64)> = sqlx::query_as(
        "SELECT repo, COUNT(*) FROM tasks WHERE status <> 'done' GROUP BY repo ORDER BY repo",
    )
    .fetch_all(&app.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(repo, open)| RepoCount { repo, open })
        .collect())
}

pub async fn repo_counts(
    Access(_): Access,
    State(app): State<AppState>,
) -> Result<Json<Vec<RepoCount>>, AppError> {
    Ok(Json(repos_with_work(&app).await?))
}
