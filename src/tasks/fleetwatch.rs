//! Pushing the tool's own timings to fleetwatch.
//!
//! **In the library rather than the binary so it can be tested against.** It
//! lived inside `src/bin/task.rs` until 2026-08-26, where nothing in `tests/`
//! could reach it — and the id it minted was never a valid ULID, so every push
//! this module ever made was refused with a 422 while every green test in the
//! repo stayed green. A private module in a binary is a module with no seam.
//!
//! ⚠ **No timer, and no prober.** Every number here came from a command
//! somebody actually ran. The service hands exactly one caller an hour the job
//! of forwarding them — see [`crate::tasks::commands::due_to_report`] — so
//! nothing happens on a day nobody uses the tracker, and fleetwatch's own
//! staleness is what says so.
//!
//! ⚠ **The token is read ONLY when the job is handed over.** It is a fleet
//! credential this CLI otherwise never touches, and reading it on every `task
//! list` would put it in the reach of every command for no reason. Once an hour
//! of active use is the whole exposure.

use anyhow::{Context, Result};
use serde_json::{Value, json};

/// Where reports go. Ingest is token-authed; the read UI is not.
const URL: &str = "https://fleetwatch.xinutec.org/api/reports";

/// The macOS Keychain item every producer on this Mac already uses.
const KEYCHAIN_ITEM: &str = "fleetwatch-ingest-token";

/// The ingest token, or nothing.
///
/// ⚠ **Absence is not an error and must never reach the caller.** A machine
/// with no token is not a broken filing; it is a machine that does not
/// report. The command that was handed the job simply does not do it.
fn token() -> Option<String> {
    let out = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            KEYCHAIN_ITEM,
            "-a",
            "fleetwatch",
            "-w",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let token = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!token.is_empty()).then_some(token)
}

/// A ULID, which fleetwatch dedupes on.
///
/// ⚠ **Minted with the crate the RECEIVER parses it with, and hand-rolling
/// it cost this whole series.** This returned `format!("{:016X}{:016X}")`
/// until 2026-08-26 — 32 hex characters, described in its own comment as
/// "ULID-shaped". A ULID is 26 characters of Crockford base32, so it never
/// was one, and `ingest` rejects a bad id with 422 before storing anything.
/// Every hourly push since the feature shipped was refused, and the local
/// recording, the carrier selection and the ablation all passed throughout
/// — none of them look at what the receiver said.
///
/// ⚠ **Random rather than derived from the numbers.** Two reports a minute
/// apart can carry identical tallies — nothing was recorded between them —
/// and a content-derived id would make the second read as a duplicate of the
/// first and be dropped, so the chart would show a gap exactly when the
/// tracker was quiet but alive. `Ulid::new` is random in its low 80 bits,
/// which is that property without a second implementation of it here.
pub fn minted() -> String {
    ulid::Ulid::new().to_string()
}

/// One check line, in the shape the ingest API takes.
fn check(label: &str, observed: String, value: f64, unit: &str, verdict: &str) -> Value {
    json!({
        "section": "tasks",
        "label": label,
        "verdict": verdict,
        "observed": observed,
        "value": value,
        "unit": unit,
    })
}

/// Turn the service's tally into checks.
///
/// ⚠ **Values, and almost no verdicts.** These distributions are days old
/// and nobody knows their shape yet; a threshold invented now would be the
/// probe's mistake again — publishing a guess as a measurement. The one
/// thing asserted is that no filing should go unchecked, because zero is
/// the only defensible expectation on that line. Everything else is a
/// number fleetwatch can chart until there is a week to derive a bound from.
pub fn checks(report: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    for line in report["commands"].as_array().unwrap_or(&Vec::new()) {
        let (Some(verb), Some(runs), Some(p90)) = (
            line["verb"].as_str(),
            line["runs"].as_u64(),
            line["p90_ms"].as_u64(),
        ) else {
            continue;
        };
        let failed = line["failed"].as_u64().unwrap_or(0);
        out.push(check(
            &format!("{verb} latency"),
            format!("{p90} ms p90 over {runs} runs, {failed} failed"),
            p90 as f64,
            "ms",
            "pass",
        ));
    }
    for line in report["checks"].as_array().unwrap_or(&Vec::new()) {
        let (Some(kind), Some(runs)) = (line["kind"].as_str(), line["runs"].as_u64()) else {
            continue;
        };
        let timeout = line["timeout"].as_u64().unwrap_or(0);
        let p90 = line["p90_ms"].as_u64().unwrap_or(0);
        out.push(check(
            &format!("{kind} check latency"),
            format!("{p90} ms p90 over {runs} runs"),
            p90 as f64,
            "ms",
            "pass",
        ));
        if kind == "filing" {
            out.push(check(
                "filings that went unchecked",
                format!("{timeout} of {runs}"),
                timeout as f64,
                "",
                if timeout > 0 { "warn" } else { "pass" },
            ));
        }
    }
    out
}

/// Send it, and never let sending it cost the command anything.
pub async fn send(http: &reqwest::Client, report: &Value) -> Result<()> {
    let Some(token) = token() else {
        return Ok(());
    };
    let checks = checks(report);
    if checks.is_empty() {
        return Ok(());
    }
    let body = json!({
        "schema": 1,
        "id": minted(),
        "collector": "task-timings",
        "collected_at": chrono::Utc::now().to_rfc3339(),
        "duration_ms": 0,
        "interval_s": report["interval_s"],
        "checks": checks,
    });
    let answer = http
        .post(URL)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .context("sending the timings to fleetwatch")?;
    // ⚠ **Silent on success, LOUD on failure, and the asymmetry is the
    // point.** This started out silent both ways, which is the failure this
    // whole path exists to end: a push nobody can see failing looks exactly
    // like a quiet day, and the numbers would stop arriving with nothing
    // anywhere saying so. It still cannot fail the command — the work is
    // done and printed by now — so the note goes to stderr and the caller
    // carries on.
    if !answer.status().is_success() {
        eprintln!(
            "(the timings did not reach fleetwatch: HTTP {})",
            answer.status()
        );
    }
    Ok(())
}
