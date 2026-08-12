//! Request bodies, refused in this service's words.
//!
//! ⚠ **A rule the service will not state is half a rule.** `NewTask::priority`
//! is the one key a body may not leave out, and the reason it is required rather
//! than defaulted is that `null` — *unassessed, nobody has judged this* — is a
//! real answer to it. Axum's `Json` extractor refuses a missing key before any
//! handler runs, with serde's own wording:
//!
//! ```text
//! 422 Failed to deserialize the JSON body into the target type:
//!     missing field `priority` at line 1 column 23
//! ```
//!
//! which names the field and stops. It reads as *you must pick a level*, which
//! is the opposite of the design, and it never mentions the escape that the
//! whole rule exists for. #724 had already set the bar on the other side of
//! this: an unknown holder answers **400**, names the holder, and says how to
//! find a real one.
//!
//! ⚠ **The absent key is found by LOOKING, not by reading serde's message.**
//! The obvious fix is to match `"missing field `priority`"` in the rejection
//! text; this parses the body to a `Value` first and asks whether the key is
//! there. The difference matters because serde's wording is not a contract — a
//! string match would go quiet on a dependency bump and silently answer the
//! plain 422 again, which is exactly the class of failure `tests/wire.rs`
//! cannot see from the outside. Asking the object is a question about the
//! request, and it has one answer forever.
//!
//! **The type is still what enforces the rule.** [`RequiredKeys`] only decides
//! what the refusal SAYS: if this list and the struct ever disagree, the
//! deserialiser refuses anyway and the caller gets serde's wording back. A
//! regression here costs a worse message, never a wrong acceptance —
//! `tests/priority.rs::filing::omitting_priority_is_refused` is what guards
//! that half, at the type, where it belongs.

use axum::Json;
use axum::extract::{FromRequest, Request};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::AppError;

/// What a body type says about the keys it will not do without.
///
/// Empty for every body where absence means *leave it alone* — which is all of
/// them but one. The guidance sits beside the field it is about, so a reader of
/// the struct sees both the rule and the sentence the caller will be shown.
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
/// Drop-in for `Json<T>` on a write route: same `Wire(value)` destructuring, and
/// every refusal comes back as this service's `{"error": …}` at a status the
/// rest of the API also uses. **400, not serde's 422**, because a body whose
/// fault can be named is one the caller can fix — the same reading
/// [`AppError::BadRequest`] already had for an unknown status word, which
/// reached that arm only because it was caught in a handler rather than in an
/// extractor. Which side of the extractor a check happens to sit on is not a
/// fact about the request.
pub struct Wire<T>(pub T);

impl<S, T> FromRequest<S> for Wire<T>
where
    T: RequiredKeys,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // Through `Json<Value>` rather than straight from the bytes, so the
        // content-type gate and the syntax errors stay axum's — only the shape
        // of the answer changes. The intermediate `Value` costs one small
        // allocation on the two write routes there are.
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
