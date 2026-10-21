//! Try to refute a review's findings before anybody edits on them.
//!
//!     falsify --tasks tasks.json < findings.json
//!
//! `--tasks` maps task ids to `{subject, body}` (from `task show <id> --json`),
//! so this is the decision in [`tasks::tasks::findings`] alone, runnable on a
//! saved corpus. Exits 1 when anything was refused, so a pipeline stops.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;

use tasks::tasks::findings::{Finding, Kind, Verdict, falsify};

#[derive(Deserialize)]
struct Raw {
    id: i64,
    kind: String,
    quote_problem: Option<String>,
    quote_resolving: Option<String>,
    quote_second: Option<String>,
}

#[derive(Deserialize)]
struct TaskText {
    subject: String,
    body: String,
}

fn kind(s: &str) -> Option<Kind> {
    Some(match s {
        "stale-layer" => Kind::StaleLayer,
        "contradiction" => Kind::Contradiction,
        "subject-stale" => Kind::SubjectStale,
        "repetition" => Kind::Repetition,
        "verbose" => Kind::Verbose,
        "info-at-risk" => Kind::InfoAtRisk,
        _ => return None,
    })
}

/// Try to refute a review's findings (JSON on stdin) before anybody edits on them.
#[derive(Parser)]
struct Cli {
    /// A map of task id to `{subject, body}`, from `task show <id> --json`.
    #[arg(long, value_name = "FILE")]
    tasks: PathBuf,
}

fn main() -> Result<()> {
    let path = Cli::parse().tasks;
    let texts: HashMap<String, TaskText> =
        serde_json::from_str(&std::fs::read_to_string(&path).context("reading --tasks")?)
            .context("--tasks must be {\"<id>\": {\"subject\":…, \"body\":…}}")?;

    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let raw: Vec<Raw> = serde_json::from_str(&input).context("findings on stdin")?;

    let (mut ok, mut refused, mut unknown) = (0, 0, 0);
    let mut why: HashMap<String, usize> = HashMap::new();
    for r in &raw {
        let Some(k) = kind(&r.kind) else {
            unknown += 1;
            continue;
        };
        // A finding whose task text is missing can't be checked: refused, not passed.
        let Some(t) = texts.get(&r.id.to_string()) else {
            refused += 1;
            *why.entry("task text not supplied".into()).or_default() += 1;
            println!("  ✗ #{}  [{}]  task text not supplied", r.id, r.kind);
            continue;
        };
        let f = Finding {
            kind: k,
            quote_problem: r.quote_problem.clone().unwrap_or_default(),
            quote_resolving: r.quote_resolving.clone(),
            quote_second: r.quote_second.clone(),
        };
        match falsify(&f, &t.subject, &t.body) {
            Verdict::Supported => ok += 1,
            Verdict::Rejected(rs) => {
                refused += 1;
                let names: Vec<String> = rs.iter().map(|r| format!("{r:?}")).collect();
                for n in &names {
                    *why.entry(n.clone()).or_default() += 1;
                }
                println!("  ✗ #{}  [{}]  {}", r.id, r.kind, names.join(", "));
            }
        }
    }

    println!(
        "\n{} findings · {ok} survive · {refused} refused",
        raw.len()
    );
    if unknown > 0 {
        println!("  {unknown} of an unrecognised kind, not checked");
    }
    let mut counts: Vec<_> = why.into_iter().collect();
    counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (name, n) in counts {
        println!("  {n:4}  {name}");
    }
    if refused > 0 {
        std::process::exit(1);
    }
    Ok(())
}
