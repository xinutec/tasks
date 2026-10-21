//! The browser's own trace: taps, navigation, what never reaches the API. A
//! report of "I pressed it and nothing happened" from a phone is otherwise
//! undiagnosable. Events go into the same log stream as the requests, so a
//! session reads as one timeline; nothing is stored.
//!
//! [`one_line`] is the security boundary, shared with the other apps' copies.

use axum::Json;
use axum::http::StatusCode;
use serde::Deserialize;

use crate::access::OwnerOnly;

/// `kind` is `nav` (no label) or `tap` (label: the control's visible text).
#[derive(Debug, Deserialize)]
pub struct TelemetryEvent {
    pub kind: String,
    pub path: String,
    #[serde(default)]
    pub label: Option<String>,
    /// The client's clock, epoch ms: the only way to order events within a batch.
    pub at: i64,
}

/// Per POST, so one request can't flood the log.
const MAX_EVENTS: usize = 100;

/// In `chars`, so no glyph is split; keeps most of a task subject.
const MAX_LABEL: usize = 160;

/// Invisible or reordering characters `char::is_control` misses: zero-width
/// characters, and bidi overrides that make a log line display something other
/// than it says (Trojan Source). A deny-list, not all of category Cf.
fn is_deceptive_format(c: char) -> bool {
    matches!(c,
        '\u{00ad}'
        | '\u{200b}'..='\u{200f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{2069}'
        | '\u{feff}'
    )
}

/// Flatten a client label to one harmless log field.
///
/// ⚠ A newline in a label would forge whole log lines. Controls become spaces,
/// whitespace collapses (which also catches U+2028/2029, not controls), and the
/// result is capped.
pub fn one_line(label: &str, max: usize) -> String {
    let unbroken: String = label
        .chars()
        .map(|c| {
            if c.is_control() || is_deceptive_format(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    unbroken
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}

/// `POST /api/telemetry`. Always 204, never retried: a trace must not get in
/// the app's way. Owner only, so a leaked agent token can't write to the log.
pub async fn record(
    OwnerOnly(user): OwnerOnly,
    Json(events): Json<Vec<TelemetryEvent>>,
) -> StatusCode {
    for e in events.into_iter().take(MAX_EVENTS) {
        let label = one_line(&e.label.unwrap_or_default(), MAX_LABEL);
        tracing::info!(
            user = %user.user_id,
            kind = %e.kind,
            path = %e.path,
            label = %label,
            at = e.at,
            "client-event"
        );
    }
    StatusCode::NO_CONTENT
}
