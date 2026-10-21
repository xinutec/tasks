//! Request bodies, refused in this service's words.
//!
//! Some keys may not be left out even where `null` is legal (`NewTask::priority`:
//! `null` means *nobody has judged this*). Axum's `Json` refuses a missing key
//! with serde's `422 … missing field`, which never names the legal answers; this
//! answers 400 and names them.
//!
//! The missing key is found by looking in the parsed `Value`, never by matching
//! serde's message, which is not a contract. [`RequiredKeys`] only shapes the
//! message: the struct still enforces the rule, so a stale list costs wording,
//! never a wrong acceptance.

use axum::Json;
use axum::extract::{FromRequest, Request};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::AppError;

/// The keys a body type will not do without. Empty where absence means *leave
/// it alone*.
pub trait RequiredKeys: DeserializeOwned {
    /// Each required key, with its legal answers written as a list: it is
    /// printed as *`key` is required: **…**. Leaving the key out is not a
    /// default.*
    fn required() -> &'static [(&'static str, &'static str)] {
        &[]
    }
}

/// Drop-in for `Json<T>` on a write route: every refusal is this service's
/// `{"error": …}` with 400, like any other [`AppError::BadRequest`].
pub struct Wire<T>(pub T);

impl<S, T> FromRequest<S> for Wire<T>
where
    T: RequiredKeys,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // Through `Json<Value>`, so the content-type and syntax checks stay axum's.
        let Json(raw) = Json::<Value>::from_request(req, state)
            .await
            .map_err(|rejected| AppError::BadRequest(rejected.body_text()))?;

        if let Value::Object(given) = &raw {
            for (key, legal) in T::required() {
                if !given.contains_key(*key) {
                    return Err(AppError::BadRequest(format!(
                        "`{key}` is required: {legal}. Leaving the key out is not a default."
                    )));
                }
            }
        }

        serde_json::from_value(raw)
            .map(Wire)
            .map_err(|e| AppError::BadRequest(e.to_string()))
    }
}
