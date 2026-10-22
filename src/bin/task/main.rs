//! `task`, the CLI a Claude session uses. It mirrors the app: the person reads a
//! phone and a session has no browser, so anything visible in one must be in the
//! other, or each works from a picture the other can't see.
//!
//! A session is identified by `$CLAUDE_CODE_SESSION_ID`, which Claude Code sets
//! in every shell; `--session` and `$TASKS_SESSION` override it, for scripts.
//! There is no anonymous mode, for reads either. Tasks are named by number, with
//! or without the `#` the digest prints.
//!
//! ```text
//! task list [--all|--mine|--pile] [--done] yours and the pile; wider; narrower; spare
//! task list --handed-out          what you filed and somebody else is holding
//! task show <id> [--previous]               one task, its prose and its history
//! task undo <id>                            put back what the last edit replaced
//! task add <subject> [--body -] [--to me|human|<session>|nobody] [--priority P3]
//! task start <id> / task done <id> [--to W] move it along
//! task drop <id>                            close it without doing it
//! task reopen <id>                          put it back to open
//! task move <id> me|human|<session>|nobody  hand it over
//! task edit <id> [--subject S] [--body -] [--priority P0]  change the words, rank it
//! task digest                              exactly what a prompt receives
//! task rename <name>                        tell the service what I call myself
//! ```

use std::io::Read;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use tasks::tasks::checks;
use tasks::tasks::commands;
use tasks::tasks::density;
use tasks::tasks::duplicates;
use tasks::tasks::fleetwatch;
use tasks::tasks::focus;
use tasks::tasks::holder::{self, Holder};
use tasks::tasks::lifecycle;
use tasks::tasks::reference::TaskRef;
use tasks::tasks::selection::{self, list_query};
use tasks::tasks::types::{AssigneeKind, Created, Priority, Status, Task};
use tasks::tasks::wait;

mod checker;
mod cli;
mod client;
mod editing;
mod filing;
mod reading;
mod render;
mod waiting;

use checker::*;
use cli::*;
use client::*;
use render::*;

/// The service, by its VPN name.
const DEFAULT_URL: &str = "https://tasks.xinutec.org";

/// A `--body` value, with `-` meaning stdin.
fn body(arg: &str) -> Result<String> {
    if arg != "-" {
        return Ok(arg.to_string());
    }
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading the body from stdin")?;
    Ok(buf)
}

/// Report what a command did, after the work and the printing, silently on
/// failure. `timings` and `checks` aren't recorded: reading the measurements is
/// not use.
async fn clocked(
    client: &Client,
    verb: &'static str,
    started: std::time::Instant,
    outcome: commands::Ended,
) {
    if matches!(verb, "timings" | "checks") {
        return;
    }
    let run = commands::Run {
        verb: verb.to_string(),
        elapsed_ms: started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32,
        outcome,
        // Always `Some`: `None` means an older client.
        waited_for_a_model: Some(WAITED_FOR_A_MODEL.load(std::sync::atomic::Ordering::Relaxed)),
    };
    let req = client
        .request(reqwest::Method::POST, "/api/commands")
        .json(&run);
    // The numbers come back only when this caller is to carry them out.
    let Ok(Some(answer)) = client.send(req).await else {
        return;
    };
    if let Some(report) = answer.get("report")
        && let Err(why) = fleetwatch::send(&client.http, report).await
    {
        // Like the status line: no cost, but no silent loss.
        eprintln!("(the timings did not reach fleetwatch: {why:#})");
    }
}

/// Exit quietly when the reader of stdout has gone (`task list | head -1`):
/// `println!` panics on a closed pipe. Resetting SIGPIPE needs `unsafe`, which
/// this crate forbids, so the panic is caught; others report as usual.
fn quiet_on_broken_pipe() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let said = info
            .payload()
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| info.payload().downcast_ref::<&str>().copied())
            .unwrap_or_default();
        if said.starts_with("failed printing to stdout") && said.contains("Broken pipe") {
            std::process::exit(0);
        }
        default(info);
    }));
}

#[tokio::main]
async fn main() -> Result<()> {
    // First statement: a session waits for parsing and setup too.
    let started = std::time::Instant::now();
    quiet_on_broken_pipe();
    let cli = Cli::parse();
    let session = cli.session.clone().or_else(session_id);
    let client = Client {
        http: reqwest::Client::builder()
            .build()
            .context("building the http client")?,
        base: cli
            .url
            .clone()
            .or_else(|| std::env::var("TASKS_URL").ok())
            .unwrap_or_else(|| DEFAULT_URL.to_string())
            .trim_end_matches('/')
            .to_string(),
        token: token(),
        called: session.as_deref().and_then(called_now),
        session,
    };
    let verb = cli.command.verb();
    let done = run(cli, &client).await;
    clocked(&client, verb, started, commands::ended(&done)).await;
    // Printed, not returned, which would print anyhow's chain (`commands::said`).
    if let Err(why) = &done {
        eprintln!("{}", commands::said(why));
        std::process::exit(1);
    }
    Ok(())
}

/// Everything the CLI does, so `main` can time it.
async fn run(cli: Cli, client: &Client) -> Result<()> {
    let client = client.clone();
    match cli.command {
        Command::List(args) => reading::list(&client, cli.json, args).await?,
        Command::Undo(args) => editing::undo(&client, cli.json, args).await?,
        Command::Show(args) => reading::show(&client, cli.json, args).await?,
        Command::Add(args) => filing::add(&client, cli.json, args).await?,
        Command::Start { id } => {
            patch(&client, cli.json, id, json!({ "status": "doing" })).await?;
        }
        Command::Done { id, to, note } => {
            // The note first, so no task closes silent.
            written(&client, &id, note.as_deref()).await?;
            let mut change = json!({ "status": "done" });
            if let Some(to) = to {
                let to = client.resolve(to).await?;
                change["assignee"] = assignee(&to, client.me()?);
            }
            patch(&client, cli.json, id, change).await?;
        }
        Command::Drop { id, reason } => {
            written(&client, &id, reason.as_deref()).await?;
            patch(&client, cli.json, id, json!({ "status": "dropped" })).await?;
        }
        Command::Reopen { id } => {
            patch(&client, cli.json, id, json!({ "status": "open" })).await?;
        }
        Command::Move { id, to } => {
            let to = client.resolve(to).await?;
            patch(
                &client,
                cli.json,
                id,
                json!({ "assignee": assignee(&to, client.me()?) }),
            )
            .await?;
        }

        Command::Wait(args) => waiting::wait(&client, args).await?,
        Command::Edit(args) => editing::edit(&client, cli.json, args).await?,
        Command::Focus(args) => waiting::focus(&client, cli.json, args).await?,
        Command::Digest => {
            let query: Vec<(String, String)> = Vec::new();
            let req = client
                .request(reqwest::Method::GET, "/api/digest")
                .query(&query);
            if cli.json {
                // Refused: `digest` is plain text by design, and JSON here would invent a shape.
                bail!(
                    "digest is plain text by design — it is exactly what a prompt \
                     receives. `task list --json` is the machine-readable list."
                );
            }
            let text = client.text(req).await?;
            let bytes = text.len();
            println!("{text}");
            // The size is the point: the per-turn cost of the whole system.
            eprintln!("\n({bytes} bytes)");
        }

        Command::Timings { days } => {
            let req = client
                .request(reqwest::Method::GET, "/api/commands")
                .query(&[("days", days)]);
            let rows = client.send(req).await?.unwrap_or(json!([]));
            let runs: Vec<commands::Ran> =
                serde_json::from_value(rows.clone()).context("reading what the commands did")?;
            emit(cli.json, &rows, || {
                if runs.is_empty() {
                    println!("nothing recorded in the last {days} days");
                    return;
                }
                for line in commands::tally(&runs) {
                    println!("{}", timed_line(&line));
                }
            });
        }

        Command::Checks(args) => reading::checks(&client, cli.json, args).await?,
        Command::Sessions { all } => {
            // Holders by default; `--all` asks for every session, to find a new one's id.
            let path = if all { "/api/sessions" } else { "/api/holders" };
            let req = client.request(reqwest::Method::GET, path);
            let rows = client.send(req).await?.unwrap_or(json!([]));
            emit(cli.json, &rows, || {
                for holder in rows.as_array().cloned().unwrap_or_default() {
                    // `open/total` (`sessions::Holder`); the id is the handle for `task move`.
                    let plate = match holder["total"].as_i64() {
                        Some(total) => format!(
                            "{:>3}/{:<4} open",
                            holder["open"].as_i64().unwrap_or(0),
                            total
                        ),
                        None => format!("{:>3} open", holder["open"].as_i64().unwrap_or(0)),
                    };
                    println!(
                        "{:<40} {:<24} {plate}",
                        holder["id"].as_str().unwrap_or(""),
                        holder["name"].as_str().unwrap_or("—"),
                    );
                }
            });
        }

        Command::Rename { name } => {
            client.writing()?;
            let session = client.session.clone().expect("writing() checked it");
            // Refused: every request sends Claude Code's name, which would undo this
            // rename. Say where to rename.
            if let Some(called) = &client.called
                && called != &name
            {
                bail!(
                    "Claude Code calls this conversation `{called}`, and that is what \
                     the service is told on every command — a rename here would be \
                     replaced by the next one. Rename the conversation itself, and \
                     this follows on its own."
                );
            }
            let req = client
                .request(reqwest::Method::PATCH, &format!("/api/sessions/{session}"))
                .json(&json!({ "name": name }));
            let answer = client.send(req).await?.unwrap_or(json!({}));
            emit(cli.json, &answer, || println!("{session} is now {name}"));
        }
    }
    Ok(())
}

/// Each task's current status, one request each: a wait's blockers have other holders.
async fn statuses(client: &Client, ids: &[TaskRef]) -> Result<Vec<(u64, Status)>> {
    let mut said = Vec::with_capacity(ids.len());
    for id in ids {
        let req = client.request(reqwest::Method::GET, &id.path());
        let task = client
            .send(req)
            .await?
            .with_context(|| format!("no such task: {id}"))?;
        let task: Task = serde_json::from_value(task)
            .context("the service answered with a task this CLI could not read")?;
        said.push((task.id, task.status));
    }
    Ok(said)
}

/// The task before its last edit. The service's 404 covers no task and nothing
/// overwritten; the message names both.
async fn fetch_previous(client: &Client, id: TaskRef) -> Result<Value> {
    let req = client.request(reqwest::Method::GET, &format!("{}/previous", id.path()));
    client
        .send(req)
        .await
        .with_context(|| {
            format!(
                "no previous version of #{} is kept: either no such task, or nothing has \
                 overwritten it since one has been stored",
                id.id()
            )
        })?
        .context("the service answered with nothing")
}

/// Record that `blocked` waits for `filed`: read, extend, write, since
/// `blocked_on` is the whole set. A failure is a note; the task is filed.
async fn unblocking(client: &Client, blocked: u64, filed: u64) -> Result<()> {
    let req = client.request(reqwest::Method::GET, &format!("/api/tasks/{blocked}"));
    let task = client.send(req).await?.context("no such task")?;
    let mut edges: Vec<u64> = task["blocked_on"]
        .as_array()
        .map(|on| on.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default();
    if edges.contains(&filed) {
        return Ok(());
    }
    edges.push(filed);
    let req = client
        .request(reqwest::Method::PATCH, &format!("/api/tasks/{blocked}"))
        .json(&json!({ "blocked_on": edges }));
    client.send(req).await?;
    Ok(())
}

/// The outcome above the body, before closing. No density read: a closing task
/// isn't about to be rewritten.
async fn written(client: &Client, id: &TaskRef, note: Option<&str>) -> Result<()> {
    let Some(note) = note else {
        return Ok(());
    };
    let note = body(note)?;
    if note.trim().is_empty() {
        bail!("--note was empty: say what came of it, or leave the flag off");
    }
    let req = client
        .request(reqwest::Method::PATCH, &format!("/api/tasks/{}", id.id()))
        .json(&json!({ "prepend": note }));
    client.send(req).await?;
    Ok(())
}

/// Change a task. A write that moved nothing says so, as a note, not an error:
/// a no-op is often right, but otherwise it looks like a real change.
async fn patch(client: &Client, json: bool, id: TaskRef, change: Value) -> Result<Value> {
    client.writing()?;
    let id = id.id();
    let req = client
        .request(reqwest::Method::PATCH, &format!("/api/tasks/{id}"))
        .json(&change);
    let task = client.send(req).await?.context("no task came back")?;
    let shown: Task = serde_json::from_value(task.clone())
        .context("the service answered with a task this CLI could not read")?;
    emit(json, &task, || {
        let unwritten = task["unwritten"]
            .as_str()
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .map(|written| lifecycle::rewrite_hint(id, written.to_utc(), chrono::Utc::now()));
        println!("{}", lifecycle::on_the_line(line(&shown), unwritten));
        if task["changed"].as_array().is_some_and(|c| c.is_empty()) {
            println!("nothing changed — it was already like that");
        }
        if let Some(was) = displaced(&task, id) {
            println!("{was}");
        }
    });
    Ok(task)
}
