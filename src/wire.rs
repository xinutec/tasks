//! Request bodies, refused in this service's words.
//!
//! ⚠ **A rule the service will not state is half a rule.** Some keys may not
//! be left out, even where `null` is a legal value — `NewTask::priority`, where
//! `null` means *nobody has judged this*. Axum's `Json` extractor refuses a
//! missing key with serde's wording (`422 … missing field`), which reads as
//! *you must pick a value* and never names the legal answers. This answers
//! **400** and names them.
//!
//! ⚠ **The absent key is found by LOOKING, not by reading serde's message.**
//! The body is parsed to a `Value` and asked whether the key is there; serde's
//! wording is not a contract, and a match on it would go quiet on a bump.
//!
//! **The type still enforces the rule.** [`RequiredKeys`] only decides what the
//! refusal SAYS: if the list and the struct disagree, the deserialiser refuses
//! anyway with serde's wording. A regression here costs a worse message, never
//! a wrong acceptance.

use axum::Json;
use axum::extract::{FromRequest, Request};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::AppError;

/// What a body type says about the keys it will not do without.
///
/// Empty for a body where absence means *leave it alone*. Declared beside the
/// struct, so its reader sees the rule and the sentence the caller is shown.
pub trait RequiredKeys: DeserializeOwned {
    /// Each key that may not be left out, with what may legally go in it.
    ///
    /// The string is read as the middle of *`key` is required: **…**. Leaving
    /// the key out is not a default.*, so write it as a list of answers rather
    /// than as a sentence.
    fn required() -> &'static [(&'static str, &'static str)] {
        &[]
    }
}

/// A JSON request body, refused with [`AppError`] rather than by the extractor.
///
/// Drop-in for `Json<T>` on a write route, with every refusal as this
/// service's `{"error": …}`. **400, not serde's 422**, like every other
/// [`AppError::BadRequest`]: which side of the extractor a check sits on is not
/// a fact about the request.
pub struct Wire<T>(pub T);

impl<S, T> FromRequest<S> for Wire<T>
where
    T: RequiredKeys,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // Through `Json<Value>`, so the content-type gate and syntax errors stay
        // axum's — only the shape of the answer changes.
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
