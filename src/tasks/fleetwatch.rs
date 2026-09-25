//! Pushing the tool's own timings to fleetwatch.
//!
//! **In the library, not the binary, so `tests/` can reach it**: a private
//! module in a binary has no seam, and a push the receiver rejects stays
//! invisible to every test. Success is a row in fleetwatch's database, so check
//! it there after changing what is sent.
//!
//! ⚠ **No timer, and no prober.** Every number came from a command somebody
//! ran, and the service hands one caller at a time the job of forwarding them
//! ([`crate::tasks::commands::due_to_report`]). On a day nobody uses the
//! tracker nothing is sent, and fleetwatch's staleness reporting says so.
//!
//! ⚠ **The token is read ONLY when the job is handed over**: it is a fleet
//! credential no other command needs.

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::tasks::checks;

/// Where reports go. Ingest is token-authed; the read UI is not.
const URL: &str = "https://fleetwatch.xinutec.org/api/reports";

/// The macOS Keychain item every producer on this Mac already uses.
const KEYCHAIN_ITEM: &str = "fleetwatch-ingest-token";

/// The ingest token, or nothing.
///
/// ⚠ **Absence is not an error and must never reach the caller**: a machine
/// with no token is one that does not report.
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
/// ⚠ **Minted with the crate the RECEIVER parses it with.** `ingest` rejects a
/// bad id before storing anything, and a "ULID-shaped" hex string is not one.
///
/// ⚠ **Random, not derived from the numbers.** Two reports can carry identical
/// tallies when nothing was recorded between them; a content-derived id would
/// drop the second as a duplicate, and the chart would show a gap exactly when
/// the tracker was quiet but alive.
pub fn minted() -> String {
    ulid::Ulid::new().to_string()
}

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
/// ⚠ **Values, and almost no verdicts.** A threshold with no distribution to
/// derive it from publishes a guess as a measurement. A verdict is given only
/// where zero is the one defensible expectation — unchecked filings, overdue
/// tasks — and the rest are numbers fleetwatch charts.
pub fn checks(report: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    let mut failed_total = 0u64;
    let mut refused_total = 0u64;
    let mut run_total = 0u64;
    let mut worst: Option<(String, u64)> = None;
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
        // ⚠ **The line above is the MIX; this one is the service.** See
        // `commands::Tally::unchecked_p90_ms` for why the mix cannot be read as
        // a latency. This one moves only for the service.
        //
        // ⚠ **Emitted only where the client said**: falling back to the mix
        // would make a second copy of the number it corrects.
        if let Some(alone) = line["unchecked_p90_ms"].as_u64() {
            let waited = line["waited"].as_u64().unwrap_or(0);
            out.push(check(
                &format!("{verb} latency, no model"),
                format!("{alone} ms p90; {waited} of {runs} waited for one"),
                alone as f64,
                "ms",
                "pass",
            ));
        }
        failed_total += failed;
        refused_total += line["refused"].as_u64().unwrap_or(0);
        run_total += runs;
        if failed > 0 && worst.as_ref().is_none_or(|(_, most)| failed > *most) {
            worst = Some((verb.to_string(), failed));
        }
    }
    // ⚠ **Failures as a VALUE**: charts are built from values, and prose in
    // a latency line is neither. One aggregate, not a series per verb, since
    // it is almost always zero.
    //
    // ⚠ **No verdict**: this is faults alone (refusals are the next line), and
    // without a normal day's count a threshold fires on noise.
    if !report["commands"]
        .as_array()
        .unwrap_or(&Vec::new())
        .is_empty()
    {
        let observed = match &worst {
            Some((verb, most)) => {
                format!("{failed_total} of {run_total} runs; most in `{verb}` ({most})")
            }
            None => format!("0 of {run_total} runs"),
        };
        out.push(check(
            "commands that failed",
            observed,
            failed_total as f64,
            "",
            "pass",
        ));
        // ⚠ **Its own line, because it is not a fault** — `commands::Ended`
        // carries why declining and failing must not share a figure.
        out.push(check(
            "commands the tool declined",
            format!("{refused_total} of {run_total} runs"),
            refused_total as f64,
            "",
            "pass",
        ));
    }
    for line in report["checks"].as_array().unwrap_or(&Vec::new()) {
        let (Some(kind), Some(runs)) = (line["kind"].as_str(), line["runs"].as_u64()) else {
            continue;
        };
        let timeout = line["timeout"].as_u64().unwrap_or(0);
        let errored = line["error"].as_u64().unwrap_or(0);
        let spoke = line["spoke"].as_u64().unwrap_or(0);
        let p90 = line["p90_ms"].as_u64().unwrap_or(0);
        out.push(check(
            &format!("{kind} check latency"),
            format!("{p90} ms p90 over {runs} runs"),
            p90 as f64,
            "ms",
            "pass",
        ));
        // ⚠ **A COUNT, with its denominator in the sentence — not a rate.** An
        // improving rate cannot say which half moved: fewer checks speaking and
        // fewer running look identical in a percentage. Spoke, quiet, timeout
        // and error sum to `runs`, so the series read against each other.
        out.push(check(
            &format!("{kind} checks that spoke"),
            format!("{spoke} of {runs}"),
            spoke as f64,
            "",
            "pass",
        ));
        // ⚠ **A timeout and an error are summed HERE and nowhere else.** They
        // have different causes and the tally keeps them apart, but both mean
        // the same thing to a reader of this line: the input was never judged.
        // `Quiet` is the one that must never join them — a check that ran and
        // had nothing to say is the opposite finding, and is its own line above.
        //
        // ⚠ **Only `filing` gets a verdict, and the asymmetry is deliberate.**
        // Zero is defensible there: an unchecked filing is how a duplicate gets
        // in. A density read is advisory and no bound has been derived for it,
        // so warning would publish a guess as a finding.
        let unanswered = timeout + errored;
        out.push(check(
            &format!("{kind} checks that never answered"),
            format!("{timeout} timed out, {errored} errored, of {runs}"),
            unanswered as f64,
            "",
            // ⚠ From [`checks::Kind`], not spelled, or a rename would silently
            // stop this warning firing.
            if kind == checks::Kind::Filing.as_str() && unanswered > 0 {
                "warn"
            } else {
                "pass"
            },
        ));
    }
    // ⚠ **The WORK**, where everything above measures the machinery. Absent
    // when the service could not count, rather than zeros.
    if let Some(work) = report.get("work").filter(|w| w.is_object()) {
        for (label, key) in [
            ("open tasks", "open"),
            ("tasks in the pile", "unheld"),
            ("tasks at P0 or P1", "urgent"),
            ("tasks blocked on open work", "blocked"),
            // The number the digest's sprawl mark exists to move.
            ("bodies carrying an unaddressed finding", "sprawling"),
        ] {
            let Some(count) = work[key].as_u64() else {
                continue;
            };
            out.push(check(label, format!("{count}"), count as f64, "", "pass"));
        }
        // ⚠ **A bound, because zero is defensible**: a deadline was set from
        // outside the tracker, and the digest and the rank already treat a
        // missed date as a change of state.
        if let Some(overdue) = work["overdue"].as_u64() {
            out.push(check(
                "tasks past their deadline",
                format!("{overdue}"),
                overdue as f64,
                "",
                if overdue > 0 { "warn" } else { "pass" },
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
    // ⚠ **Silent on success, LOUD on failure**: a push nobody sees failing
    // looks like a quiet day. It still cannot fail the command — the work is
    // done — so the note goes to stderr.
    if !answer.status().is_success() {
        eprintln!(
            "(the timings did not reach fleetwatch: HTTP {})",
            answer.status()
        );
    }
    Ok(())
}
