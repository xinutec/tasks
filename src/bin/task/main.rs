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
mod render;

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
        Command::List {
            all,
            mine,
            pile,
            handed_out,
            done,
            to,
        } => {
            // Through the same path as `move`, so `--to x` and `move <id> x` agree.
            let holder = match to {
                Some(to) => Some(client.resolve(to).await?),
                None => None,
            };
            let me = client.me().ok();
            let asked = holder.as_ref().map(|to| match to {
                Target::Nobody => selection::Holder::Nobody,
                Target::Person(id) => selection::Holder::Person(id),
                Target::Session(id) => selection::Holder::Session(id),
                // `--to me` is `--mine`.
                Target::Me => selection::Holder::Session(me.unwrap_or_default()),
            });
            let query = list_query(
                all,
                mine,
                pile,
                handed_out,
                done,
                client.session.as_deref(),
                asked,
            )?;
            let req = client
                .request(reqwest::Method::GET, "/api/tasks")
                .query(&query);
            let tasks = client.send(req).await?.unwrap_or(json!([]));
            // Decoded outside the closure, so an unreadable list is an error, not a wrong row.
            let shown: Vec<Task> = serde_json::from_value(tasks.clone())
                .context("the service answered with a list this CLI could not read")?;
            emit(cli.json, &tasks, || {
                if shown.is_empty() {
                    // Which question came back empty: an empty pile and an empty plate differ.
                    println!(
                        "{}",
                        if pile {
                            "the pile is empty"
                        } else if handed_out {
                            // Good news: everything handed out is closed.
                            "nothing you handed out is still open"
                        } else {
                            "nothing open"
                        }
                    );
                }
                for task in &shown {
                    println!("{}", line(task));
                }
            });
        }

        Command::Show {
            id,
            body: only_body,
            previous,
        } if previous => {
            let was = fetch_previous(&client, id).await?;
            if only_body {
                print!("{}", was["body"].as_str().unwrap_or_default());
                return Ok(());
            }
            emit(cli.json, &was, || {
                // The header says when this version was replaced.
                println!(
                    "#{} as it stood until {} ({})",
                    id.id(),
                    was["at"].as_str().unwrap_or(""),
                    was["actor"].as_str().unwrap_or("")
                );
                println!("{}", was["subject"].as_str().unwrap_or(""));
                let body = was["body"].as_str().unwrap_or("").trim();
                if !body.is_empty() {
                    println!("\n{body}");
                }
                println!("\nput it back: task undo {}", id.id());
            });
        }

        Command::Undo { id, anyway } => {
            client.writing()?;
            let was = fetch_previous(&client, id).await?;
            // Read first, so a refusal costs nothing; `mine` is the service's answer on
            // stored identity.
            if !anyway {
                let was: tasks::tasks::types::Revision = serde_json::from_value(was.clone())
                    .context("the service's previous version did not parse")?;
                if tasks::tasks::undo::needs_saying(&was) {
                    return Err(commands::declined(tasks::tasks::undo::refusal(
                        &was,
                        id.id(),
                    )));
                }
            }
            // `replace_body`: see `Change::replace_body`.
            patch(
                &client,
                cli.json,
                id,
                json!({
                    "subject": was["subject"],
                    "body": was["body"],
                    "replace_body": true,
                }),
            )
            .await?;
        }

        Command::Show {
            id,
            body: only_body,
            previous: _,
        } => {
            let req = client.request(reqwest::Method::GET, &id.path());
            let task = client.send(req).await?.context("no such task")?;
            if only_body {
                // Exactly as stored, no newline added or removed, so it diffs cleanly.
                print!("{}", task["body"].as_str().unwrap_or_default());
                return Ok(());
            }
            let shown: Task = serde_json::from_value(task.clone())
                .context("the service answered with a task this CLI could not read")?;
            emit(cli.json, &task, || {
                println!("{}", line(&shown));
                let body = task["body"].as_str().unwrap_or("").trim();
                if !body.is_empty() {
                    println!("\n{body}");
                }
                // Between body and history: it is about the body. Shown until an edit shrinks it.
                if let Some(said) = task["sprawl_said"].as_str().map(str::trim)
                    && !said.is_empty()
                {
                    println!("\na model read this body, and it is a guess:");
                    for finding in said.lines().map(str::trim).filter(|l| !l.is_empty()) {
                        println!("  {finding}");
                    }
                    println!(
                        "  `task edit {} --body -` is how it gets rewritten.",
                        id.id()
                    );
                }
                if let Some(events) = task["events"].as_array()
                    && !events.is_empty()
                {
                    println!("\nhistory");
                    for event in events {
                        println!(
                            "  {}  {}  {}  {}",
                            event["at"].as_str().unwrap_or(""),
                            event["actor"].as_str().unwrap_or(""),
                            event["kind"].as_str().unwrap_or(""),
                            event["detail"].as_str().unwrap_or("")
                        );
                    }
                }
            });
        }

        Command::Add {
            subject,
            body: raw,
            to,
            spare,
            priority,
            // Handled by clap's group: `--unassessed` is the absence of `--priority`.
            unassessed: _,
            blocked_on,
            blocks,
            due,
            repo,
            project,
            subject_flag,
            not_duplicate_of,
            no_duplicate_check,
            check_only,
        } => {
            if repo.is_some() || project.is_some() {
                return Err(commands::declined(
                    "a task has no repo and no project — the field went in migration 0004, \
                     and the holder is the whole of an assignment. What this touches goes in \
                     the subject, where every list shows it: `task list` prints one line per \
                     task and that line is all most sessions will read.",
                ));
            }
            let subject = match (subject, subject_flag) {
                (Some(subject), None) => subject,
                (None, Some(said)) => {
                    return Err(commands::declined(format!(
                        "the subject is the first argument, not a flag: \
                         `task add {said:?} --priority P2`. Nothing was filed."
                    )));
                }
                (Some(_), Some(_)) => {
                    return Err(commands::declined(
                        "the subject was given twice, as the argument and as `--subject`. \
                         Nothing was filed, because there is no way to tell which one you meant.",
                    ));
                }
                (None, None) => {
                    return Err(commands::declined(
                        "a filing needs a subject: `task add \"one line\" --priority P2`.",
                    ));
                }
            };
            // Before the duplicate check, so a filing the service would refuse spends no
            // model call (`repo::spare_note`). Trimmed as the service trims.
            let reason = spare
                .as_deref()
                .map(str::trim)
                .filter(|why| !why.is_empty());
            match holder::pile_verdict(matches!(to, Some(To::Nobody)), reason.is_some()) {
                holder::PileVerdict::Fits => {}
                refused => return Err(commands::declined(refused.said_to_cli())),
            }
            client.writing()?;
            // Skipped only for an override the service will accept: a recent refusal of
            // this subject, and naming every task it named. Otherwise the check runs, so
            // asking to skip first time gains nothing.
            let asked = !not_duplicate_of.is_empty() || no_duplicate_check;
            let skip = match (asked, check_only) {
                (false, _) | (_, true) => false,
                (true, false) => match licence(&client, &subject).await {
                    Ok(refused) => {
                        match checks::overrule(true, &not_duplicate_of, refused.as_deref()) {
                            checks::Overrule::Skip => true,
                            checks::Overrule::Unnamed(missing) => {
                                return Err(commands::declined(checks::unnamed(&missing)));
                            }
                            checks::Overrule::Check => {
                                eprintln!(
                                    "(nothing has refused this subject yet, so the duplicate check runs)"
                                );
                                false
                            }
                        }
                    }
                    Err(why) => {
                        eprintln!(
                            "(could not ask whether a refusal licenses this, so the check runs: {why:#})"
                        );
                        false
                    }
                },
            };
            // Two refusing questions before the POST: an open task with this exact
            // subject, and a model seeing the same problem in other words. `--check-only`
            // asks both and stops. An unreadable list costs a note, never a filing.
            let (corpus, mine) = match skip {
                true => (Vec::new(), Vec::new()),
                false => open_now(&client).await.unwrap_or_else(|why| {
                    eprintln!("(duplicate check did not run: {why:#})");
                    (Vec::new(), Vec::new())
                }),
            };
            if let Some(already) = duplicates::same_subject(&subject, &corpus) {
                // Recorded like a model's refusal, so its override is accepted.
                recorded(
                    &client,
                    checks::Run {
                        kind: checks::Kind::Collision,
                        task_id: None,
                        input_chars: 0,
                        accreted: None,
                        elapsed_ms: 0,
                        outcome: checks::Outcome::Spoke,
                        subject_key: Some(checks::subject_key(&subject)),
                        matched: vec![already],
                        said: None,
                    },
                )
                .await;
                return Err(commands::declined(duplicates::collision(already)));
            }
            // Before the POST: a refusal afterwards is not one. Edges narrow only what the
            // model reads, after the exact check over everything.
            let candidates = duplicates::edged(&corpus, &blocked_on, &blocks);
            // The closed list isn't narrowed: nothing waits on a closed task.
            let (settled, unread) = match skip {
                true => (Vec::new(), 0),
                false => settled_now(&client).await,
            };
            if !skip {
                match already_filed(&client, &candidates, &settled, &subject).await {
                    Ok(found) if !found.is_empty() => {
                        let (open, over) = duplicates::split(&found, &settled);
                        // Open wins when both are named: folding into live work beats reopening.
                        // `declined`, never `bail!`, or the commonest refusal prints as a failure.
                        let said = match open.is_empty() {
                            false => duplicates::refusal(&open, &candidates, &mine),
                            true => duplicates::reopen_instead(&over, settled.len(), unread),
                        };
                        return Err(commands::declined(said));
                    }
                    Ok(_) => {}
                    Err(why) => eprintln!("(duplicate check did not run: {why:#})"),
                }
            }
            // Only reached when nothing was named.
            if check_only {
                println!("NONE — nothing open or closed was named");
                return Ok(());
            }
            let mut payload = json!({
                "subject": subject,
                "body": raw.as_deref().map(body).transpose()?.unwrap_or_default(),
                // Always sent, so the service can tell a skip from an old client.
                "checked": !skip,
            });
            if skip {
                payload["not_duplicate_of"] = json!(not_duplicate_of);
            }
            // Always present, null when unassessed; the service refuses a missing key.
            payload["priority"] = match priority {
                Some(priority) => json!(priority.as_str()),
                None => Value::Null,
            };
            if !blocked_on.is_empty() {
                payload["blocked_on"] = json!(blocked_on);
            }
            if let Some(due) = due {
                payload["due"] = json!(due.to_string());
            }
            if let Some(to) = to {
                let to = client.resolve(to).await?;
                payload["assignee"] = assignee(&to, client.me()?);
            }
            // As given: refused above if it didn't fit, and by the service's type.
            if let Some(spare) = spare {
                payload["spare"] = json!(spare);
            }
            let req = client
                .request(reqwest::Method::POST, "/api/tasks")
                .json(&payload);
            let task = client.send(req).await?.context("no task came back")?;
            let created: Created = serde_json::from_value(task.clone())
                .context("the service answered with a task this CLI could not read")?;
            emit(cli.json, &task, || {
                let now = chrono::Utc::now();
                let hints = created
                    .closed
                    .iter()
                    .map(|closed| lifecycle::reopen_hint(closed, created.task.id, now));
                println!("{}", lifecycle::on_the_line(line(&created.task), hints));
            });
            // After the POST, which gives the id. Recorded on the other task: `blocked_on`
            // belongs to the waiter.
            if let Some(filed) = task["id"].as_u64() {
                for blocked in &blocks {
                    if let Err(why) = unblocking(&client, *blocked, filed).await {
                        eprintln!(
                            "(filed, but #{blocked} was not marked as waiting for it: {why:#})"
                        );
                    }
                }
            }
        }

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

        Command::Wait { ids, period } => {
            // Checked first, so a typo is a refusal, not a day-long wait.
            let bound = focus::parse(&period)?
                .to_std()
                .context("a wait cannot be negative")?;
            let asked: Vec<u64> = ids.iter().map(TaskRef::id).collect();
            // A missing task fails the first pass rather than being polled.
            let started = std::time::Instant::now();
            let verdict = loop {
                let verdict = wait::verdict(&statuses(&client, &ids).await?);
                if !matches!(verdict, wait::Verdict::Waiting(_)) {
                    break verdict;
                }
                let waited = started.elapsed();
                if waited >= bound {
                    break verdict;
                }
                tokio::time::sleep(wait::interval(waited).min(bound - waited)).await;
            };
            let said = wait::said(&verdict, &asked);
            if verdict == wait::Verdict::Done {
                println!("{said}");
            } else {
                bail!(said);
            }
        }

        Command::Edit {
            id,
            subject,
            body: raw,
            prepend,
            append,
            no_density_check,
            replace_body,
            priority,
            blocked_on,
            unblock,
            due,
            no_due,
        } => {
            let mut change = json!({});
            if let Some(priority) = priority {
                change["priority"] = json!(priority.as_str());
            }
            // An empty list clears, so `--unblock` and `--blocked-on` share the field.
            if unblock {
                change["blocked_on"] = json!([] as [u64; 0]);
            } else if !blocked_on.is_empty() {
                change["blocked_on"] = json!(blocked_on);
            }
            // A date has no empty value, so clearing needs its own field.
            if no_due {
                change["clear_due"] = json!(true);
            } else if let Some(due) = due {
                change["due"] = json!(due.to_string());
            }
            if let Some(subject) = subject {
                change["subject"] = json!(subject);
            }
            if let Some(raw) = raw {
                change["body"] = json!(body(&raw)?);
                // Only with a body, so the flag can't linger on a line that writes none.
                if replace_body {
                    change["replace_body"] = json!(true);
                }
            }
            // One stdin, so only one of these may read it; a second would read nothing.
            if prepend.as_deref() == Some("-") && append.as_deref() == Some("-") {
                bail!("only one of --prepend and --append can read stdin — give the other inline");
            }
            if let Some(text) = prepend {
                change["prepend"] = json!(body(&text)?);
            }
            if let Some(text) = append {
                change["append"] = json!(body(&text)?);
            }
            if change.as_object().is_none_or(|o| o.is_empty()) {
                bail!("nothing to change: pass --subject or --body");
            }
            let updated = patch(&client, cli.json, id, change).await?;
            // After the write: advice, never a refusal.
            if !no_density_check && !cli.json {
                accreting(&client, id, &updated).await;
            }
        }

        Command::Focus { ids, period, clear } => {
            if clear {
                let req = client.request(reqwest::Method::DELETE, "/api/focus");
                let was = client.send(req).await?.unwrap_or(json!({}));
                emit(cli.json, &was, || match was["was"].is_null() {
                    false => println!("focus ended — your prompt shows everything open again"),
                    true => println!("there was no focus on"),
                });
            } else if ids.is_empty() {
                let req = client.request(reqwest::Method::GET, "/api/focus");
                let focus = client.send(req).await?.unwrap_or(Value::Null);
                emit(cli.json, &focus, || match parse_focus(&focus) {
                    Some(focus) => println!("{}", describe(&focus)),
                    None => println!(
                        "not focused — your prompt shows everything open. \
                         `task focus <id>… --for 4h` narrows it."
                    ),
                });
            } else {
                let period = period.context(
                    "how long? `--for 4h`. There is no default: the expiry is what makes \
                     hiding an open task safe.",
                )?;
                let period = focus::parse(&period)?;
                let body = json!({
                    "tasks": ids.iter().map(|id| id.id()).collect::<Vec<_>>(),
                    "minutes": period.num_minutes(),
                });
                let req = client
                    .request(reqwest::Method::POST, "/api/focus")
                    .json(&body);
                let focus = client.send(req).await?.unwrap_or(Value::Null);
                emit(cli.json, &focus, || match parse_focus(&focus) {
                    Some(focus) => println!("{}", describe(&focus)),
                    // A 2xx this can't read is an API disagreement; never report "focused"
                    // unconfirmed.
                    None => println!("the service accepted the focus but did not describe it"),
                });
            }
        }

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

        Command::Checks { days } => {
            let req = client
                .request(reqwest::Method::GET, "/api/checks")
                .query(&[("days", days)]);
            let rows = client.send(req).await?.unwrap_or(json!([]));
            let runs: Vec<checks::Ran> =
                serde_json::from_value(rows.clone()).context("reading what the checks did")?;
            // Per session: refusals and overrides.
            let req = client
                .request(reqwest::Method::GET, "/api/checks/overrules")
                .query(&[("days", days)]);
            let overrules: Vec<checks::Overrules> = match client.send(req).await? {
                Some(rows) => serde_json::from_value(rows).context("reading the overrides")?,
                None => Vec::new(),
            };
            emit(cli.json, &rows, || {
                if runs.is_empty() {
                    println!("nothing recorded in the last {days} days");
                    return;
                }
                for line in checks::tally(&runs) {
                    println!("{}", tallied(&line));
                }
                for who in &overrules {
                    println!(
                        "{:24} refused {:3}, filed past {:3}",
                        who.session, who.refused, who.overruled
                    );
                }
            });
        }
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
