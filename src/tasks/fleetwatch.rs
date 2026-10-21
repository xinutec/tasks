//! Pushing the tool's own timings to fleetwatch.
//!
//! In the library so `tests/` can reach it; success is a row in fleetwatch's
//! database, so check there after changing what is sent. No timer: numbers come
//! from commands people ran, forwarded by one caller at a time
//! ([`crate::tasks::commands::due_to_report`]). The fleet token is read only then.

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::tasks::checks;

/// Token-authed ingest.
const URL: &str = "https://fleetwatch.xinutec.org/api/reports";

/// The macOS Keychain item every producer on this Mac already uses.
const KEYCHAIN_ITEM: &str = "fleetwatch-ingest-token";

/// The ingest token. None means this machine doesn't report: not an error.
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

/// A ULID, which fleetwatch dedupes on, from the crate it parses with. Random,
/// not derived from content: two identical quiet tallies would otherwise collapse
/// into one, leaving a gap exactly when the tracker was quiet but alive.
pub fn minted() -> String {
    ulid::Ulid::generate().to_string()
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

/// Turn the service's tally into checks: values, and a verdict only where zero
/// is the one defensible expectation (unchecked filings, overdue tasks). A
/// threshold with no distribution behind it is a guess.
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
        // The line above mixes checked and unchecked commands
        // (`commands::Tally::unchecked_p90_ms`); this one is the service alone, sent only
        // when the client reported it.
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
    // Faults as one value, no verdict: there is no normal day to set a bound from.
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
        // Refusals apart from faults (`commands::Ended`).
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
        // Counts, not a rate: a rate can't say which half moved. Spoke, quiet,
        // timeout and error sum to `runs`.
        out.push(check(
            &format!("{kind} checks that spoke"),
            format!("{spoke} of {runs}"),
            spoke as f64,
            "",
            "pass",
        ));
        // Timeouts and errors summed: either way the input was never judged. Only
        // `filing` gets a verdict, because an unchecked filing is how a duplicate gets
        // in; no bound has been derived for density.
        let unanswered = timeout + errored;
        out.push(check(
            &format!("{kind} checks that never answered"),
            format!("{timeout} timed out, {errored} errored, of {runs}"),
            unanswered as f64,
            "",
            // From [`checks::Kind`], so a rename can't silence it.
            if kind == checks::Kind::Filing.as_str() && unanswered > 0 {
                "warn"
            } else {
                "pass"
            },
        ));
    }
    // The backlog itself. Absent, not zero, when the service couldn't count.
    if let Some(work) = report.get("work").filter(|w| w.is_object()) {
        for (label, key) in [
            ("open tasks", "open"),
            ("tasks in the pile", "unheld"),
            ("tasks at P0 or P1", "urgent"),
            ("tasks blocked on open work", "blocked"),
            ("bodies carrying an unaddressed finding", "sprawling"),
        ] {
            let Some(count) = work[key].as_u64() else {
                continue;
            };
            out.push(check(label, format!("{count}"), count as f64, "", "pass"));
        }
        // A bound: a missed deadline is a change of state everywhere else too.
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
    // Loud on failure (a silent failed push looks like a quiet day), but never fails the command.
    if !answer.status().is_success() {
        eprintln!(
            "(the timings did not reach fleetwatch: HTTP {})",
            answer.status()
        );
    }
    Ok(())
}
