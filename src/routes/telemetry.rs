//! Client activity trace: what the browser sees and the API does not.
//!
//! **Why this exists, and it is not analytics.** The per-request trace already
//! logs every API call, and that is not sufficient: a tap that hit a disabled
//! control, a filter that quietly matched nothing, a move that never fired —
//! none of it reaches the server, so none of it can be diagnosed afterwards from
//! a report like "I pressed it and nothing happened". This app is used from a
//! phone, where that report is the only one there is.
//!
//! The events fold into the **same** log stream as the API requests, so a
//! session reads as one timeline: `client-event kind=nav path=/t/12`, then
//! `client-event kind=tap label="Done"`, then the `PATCH /api/tasks/12 200`
//! the tap caused.
//!
//! **There is no storage here.** These are logs, not data. The endpoint moves
//! the client's events into the backend log and forgets them.
//!
//! The part to read before changing anything is [`one_line`]: it is the
//! security boundary, shared with the other apps' copies of this endpoint.

use axum::Json;
use axum::http::StatusCode;
use serde::Deserialize;

use crate::access::OwnerOnly;

/// One thing that happened in the client.
///
/// `kind` is `nav` for a route change, where `label` is absent, or `tap` for a
/// control, where `label` is its visible text, verbatim.
#[derive(Debug, Deserialize)]
pub struct TelemetryEvent {
    pub kind: String,
    pub path: String,
    #[serde(default)]
    pub label: Option<String>,
    /// The client's clock, in epoch milliseconds.
    ///
    /// Kept because a batch arrives all at once, so the server's receive time
    /// cannot order the events inside it and the client's can.
    pub at: i64,
}

/// Most events accepted from one POST.
///
/// The real client batches a handful at a time; this stops a buggy or hostile
/// one turning a single request into a log flood.
const MAX_EVENTS: usize = 100;

/// Longest label kept, in characters.
///
/// Labels are verbatim UI text, often a task subject, so this keeps most of
/// one. Counted in `chars`, so a multi-byte glyph is never split.
const MAX_LABEL: usize = 160;

/// Format characters that are invisible, or that reorder what is displayed.
///
/// `char::is_control` covers category Cc only, and std has no Unicode category
/// table, so these are named explicitly:
///
/// - **Zero-width characters** (U+200B, U+FEFF, the word joiners) are invisible,
///   so a label made of them reads as empty while occupying the whole cap.
/// - **Bidi overrides** (U+202A–202E, U+2066–2069) reorder the *rendering* of
///   the text around them. A log line containing one can be made to display
///   something other than what it says — the Trojan Source trick, pointed at the
///   record rather than at source code.
///
/// A deny-list of what can deceive, not all of category Cf: a Unicode tables
/// crate would be disproportionate. Stated so the limit is known.
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

/// Flatten a client-supplied label to a single harmless log field.
///
/// **The security boundary of the endpoint, not tidiness.** A label is written
/// into a log line as `label=…`, so one containing a newline forges *whole log
/// lines* — attributed to someone else, or to another component — and the log
/// stops being evidence.
///
/// Control characters become spaces, runs of whitespace collapse, and the result
/// is capped. `char::is_control` covers C0 and C1 but *not* U+2028 and U+2029,
/// which end a line in some renderers; `split_whitespace` catches those, so the
/// two passes together cover both.
///
/// Public so `tests/telemetry.rs` can exercise it: its input is the attacker's.
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

/// `POST /api/telemetry` — fold the client's events into the log stream.
///
/// Always 204. Telemetry is best-effort: the client neither reads the response
/// nor retries, because a trace that interferes with the app it observes is
/// worse than no trace.
///
/// Owner-gated: a session has no browser to trace, and the gate stops a leaked
/// agent token writing lines into the log.
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
