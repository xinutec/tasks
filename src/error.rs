//! Application error type with an axum `IntoResponse` so handlers can `?`.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// No usable credential — or one that stopped short of saying who it is.
    ///
    /// ⚠ The message is carried rather than fixed because "not authenticated"
    /// is actively misleading in one of the two cases: a caller whose agent
    /// token is *correct* but who sent no `X-Session-Id` gets sent to check the
    /// one thing that was right.
    #[error("{0}")]
    Unauthorized(&'static str),

    #[error("not authorized")]
    Forbidden,

    #[error("not found")]
    NotFound,

    /// A request that cannot be honoured as asked — an unknown status word, a
    /// subject too long for the one line it has to fit in. The message is shown
    /// to the caller, because the caller is the one who can fix it.
    #[error("{0}")]
    BadRequest(String),

    /// Anything unexpected → 500; body is generic, detail is logged.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Other(e.into())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        AppError::Other(e.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            AppError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, self.to_string()),
            AppError::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::Other(e) => {
                tracing::error!("internal error: {e:#}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal error".to_string(),
                )
            }
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}
