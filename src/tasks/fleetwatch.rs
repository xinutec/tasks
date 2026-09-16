//! Pushing the tool's own timings to fleetwatch.
//!
//! **In the library rather than the binary so it can be tested against.** As a
//! private module inside the binary nothing in `tests/` could reach it, and it
//! spent its whole life pushing an id the receiver rejected while every test in
//! the repo stayed green. A private module in a binary is a module with no seam.
//!
//! ⚠ **It WORKED before it shipped, which is the part worth remembering.** One
//! report landed, pushed by a development build; the id format was widened just
//! before the commit and every push after that was refused. The evidence of
//! success was a row in somebody else's database that nobody re-read, so the
//! last edit before shipping went unchecked.
//!
//! ⚠ **No timer, and no prober.** Every number here came from a command somebody
//! actually ran, and the service hands one caller at a time the job of
//! forwarding them — see [`crate::tasks::commands::due_to_report`]. Nothing
//! happens on a day nobody uses the tracker, and fleetwatch's own staleness
//! reporting is what says so.
//!
//! ⚠ **The token is read ONLY when the job is handed over.** It is a fleet
//! credential this CLI otherwise never touches, and reading it on every `task
//! list` would put it in reach of every command for no reason.

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::tasks::checks;

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
/// ⚠ **Minted with the crate the RECEIVER parses it with, and hand-rolling it
/// cost this whole series.** A hex string described in its own comment as
/// "ULID-shaped" is not a ULID, and `ingest` rejects a bad id before storing
/// anything. Every push was refused while the local recording, the carrier
/// selection and the ablation all passed — none of them look at what the
/// receiver said.
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
        // ⚠ **Emitted only where the client said**, so an older CLI's rows do
        // not quietly become a second copy of the number above. A figure that
        // falls back to the value it is meant to correct looks like the fix
        // working, which is worse than no figure.
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
    // ⚠ **`failed` was already in the sentence above and in no VALUE**, so a
    // verb that started failing could only be read by hovering a latency line —
    // charts and staleness bands are built from values, and prose is neither.
    // One aggregate rather than a line per verb: eight more series to say a
    // number that is almost always zero would crowd out the ones that move.
    //
    // ⚠ **No verdict, deliberately.** This figure is faults alone — refusals
    // land in the line below, which is what the `Refused` split is for — but
    // nobody has measured what a normal day's fault count is, and a threshold
    // picked without that fires on ordinary noise and trains everyone to ignore
    // the one line that is supposed to mean something.
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
        //
        // ⚠ **Both series move while old rows age out**, because rows written
        // before the split said `error` for both and are not re-attributed. Say
        // so on the graph rather than in a comment nobody reading it will see.
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
        // ⚠ **A COUNT, with its denominator in the sentence — not a rate.** A
        // speak rate that improves says nothing about which half moved: fewer
        // checks speaking and fewer checks running look identical in a
        // percentage and mean opposite things. The counts compose — spoke,
        // quiet, timeout and error sum to `runs` — so the series can be read
        // against each other.
        //
        // This is the number that refutes turning the density advice into a
        // refusal: it speaks on most of what it reads. Measured by hand out of
        // `check_run` the first time, because nothing charted it.
        out.push(check(
            &format!("{kind} checks that spoke"),
            format!("{spoke} of {runs}"),
            spoke as f64,
            "",
            "pass",
        ));
        // ⚠ **Every kind, and it used to be `filing` alone**, so the one number
        // saying how often a check simply does not happen was invisible for the
        // kind that runs most.
        //
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
            // ⚠ From [`checks::Kind`], not spelled: this is the one place the
            // filing check's name is compared outside the module that defines
            // it, and a rename there would silently stop this warning firing.
            if kind == checks::Kind::Filing.as_str() && unanswered > 0 {
                "warn"
            } else {
                "pass"
            },
        ));
    }
    // ⚠ **The WORK.** Everything above measures the tracker's machinery; this
    // measures what it is holding.
    // Absent when the service could not count — a section that reported zeros on
    // a failed query would publish "the backlog is clear" as a finding.
    if let Some(work) = report.get("work").filter(|w| w.is_object()) {
        for (label, key) in [
            ("open tasks", "open"),
            ("tasks in the pile", "unheld"),
            ("tasks at P0 or P1", "urgent"),
            ("tasks blocked on open work", "blocked"),
            // The number the digest mark exists to move. Uncharted, whether the
            // mark changes any behaviour is unanswerable.
            ("bodies carrying an unaddressed finding", "sprawling"),
        ] {
            let Some(count) = work[key].as_u64() else {
                continue;
            };
            out.push(check(label, format!("{count}"), count as f64, "", "pass"));
        }
        // ⚠ **The one line here that claims a bound, on the same ground the
        // filing line does: zero is defensible.** A deadline is the only thing
        // in this tracker that anybody outside it set, the digest already SHOUTS
        // `OVERDUE`, and the rank escalates to `P0` a week out — three places
        // that already treat a missed date as a state change rather than a
        // level. The rest stay values: their distributions have no derived
        // bounds and inventing one now would publish a guess as a measurement.
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
