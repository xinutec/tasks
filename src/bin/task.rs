//! `task` — the CLI half of this service, and the half a Claude session uses.
//!
//! **It mirrors the app, and that is a rule rather than a convenience.** Pippijn
//! reads the list on a phone; a session has no browser at all. If the two
//! surfaces diverge, one party is working from a picture of the work the other
//! cannot see — which is the exact failure this service exists to prevent.
//! Anything that becomes visible in the UI gets a line here.
//!
//! **Who am I?** A session is identified by the CLI's own session id, which it
//! learns from the prompt hook and passes as `TASKS_SESSION` (or `--session`).
//! Without one, every command still reads, and none of them writes: filing work
//! as nobody-in-particular is worse than refusing.
//!
//! ```text
//! task list [--repo R] [--mine] [--done]   what is open
//! task show <id>                            one task, its prose and its history
//! task add <subject> [--repo R] [--body -] [--to me|<session>|nobody]
//! task start <id> / task done <id>          move it along
//! task move <id> me|<session>|nobody        hand it over
//! task edit <id> [--subject S] [--body -]   change the words
//! task digest [--repo R]                    exactly what a prompt receives
//! task rename <name>                        tell the service what I call myself
//! ```

use std::io::Read;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};

/// Where the service lives. The VPN name, because that is the only place it is.
const DEFAULT_URL: &str = "https://tasks.xinutec.org";

#[derive(Parser)]
#[command(
    name = "task",
    about = "The work Claude sessions and Pippijn hand between each other"
)]
struct Cli {
    /// Base URL of the service. Defaults to $TASKS_URL, then the VPN name.
    #[arg(long, global = true)]
    url: Option<String>,
    /// This conversation's CLI session id. Defaults to $TASKS_SESSION.
    #[arg(long, global = true)]
    session: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// What is open.
    List {
        /// Comma-separated repositories. Absent means every one.
        #[arg(long)]
        repo: Option<String>,
        /// Only what this session is holding.
        #[arg(long)]
        mine: bool,
        /// Include finished tasks.
        #[arg(long)]
        done: bool,
    },
    /// One task, with its prose and its history.
    Show { id: u64 },
    /// File a task.
    Add {
        subject: String,
        #[arg(long)]
        repo: Option<String>,
        /// The body. `-` reads stdin, which is how a session writes a long one
        /// without fighting shell quoting.
        #[arg(long)]
        body: Option<String>,
        /// Who it is for: `me`, `nobody`, or a session id.
        #[arg(long)]
        to: Option<To>,
    },
    /// Mark a task as being worked on.
    Start { id: u64 },
    /// Mark a task finished.
    Done { id: u64 },
    /// Hand a task over: `me`, `nobody`, or a session id.
    Move { id: u64, to: To },
    /// Change a task's words.
    Edit {
        id: u64,
        #[arg(long)]
        subject: Option<String>,
        /// `-` reads stdin.
        #[arg(long)]
        body: Option<String>,
    },
    /// Exactly what a prompt receives — for checking the cost, not for reading.
    Digest {
        #[arg(long)]
        repo: Option<String>,
    },
    /// Every session known, and how much each is holding.
    Sessions,
    /// Tell the service what this session now calls itself.
    Rename { name: String },
}

/// The shared secret, from the environment or the file the Mac keeps it in.
///
/// Never on argv: a token in a command line is in every process listing on the
/// machine and in the transcript of the session that typed it.
///
/// dev-lint: allow-env-contract — read by THIS BINARY, which is the Mac's CLI
/// and is not what the container runs. The env-contract join reads the whole
/// repo's sources against the deployment's env, and the deployment must not
/// supply this: the pod is the thing the token authenticates *to*, so a copy of
/// it inside the pod would be a credential held by its own verifier for no
/// caller. `src/main.rs` reads `AGENT_TOKEN` instead, which the manifest does
/// supply.
fn token() -> Option<String> {
    if let Ok(value) = std::env::var("TASKS_TOKEN")
        && !value.trim().is_empty()
    {
        return Some(value.trim().to_string());
    }
    let path = std::path::Path::new(&std::env::var("HOME").ok()?)
        .join(".config")
        .join("tasks")
        .join("token");
    std::fs::read_to_string(path)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

struct Client {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
    session: Option<String>,
}

impl Client {
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut req = self.http.request(method, format!("{}{path}", self.base));
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        if let Some(session) = &self.session {
            req = req.header("X-Session-Id", session);
        }
        req
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Option<Value>> {
        let res = req.send().await.context("reaching the tasks service")?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        if !status.is_success() {
            // The service's own message, not a status code: it says which field
            // was wrong, and that is the whole value of the round trip.
            let said = match serde_json::from_str::<Value>(&body) {
                Ok(parsed) => parsed["error"].as_str().map(str::to_string).unwrap_or(body),
                // Not JSON at all — an ingress page, a proxy timeout. The body
                // IS the message there, so nothing is being defaulted away.
                Err(_) => body,
            };
            bail!("{status}: {said}");
        }
        if body.trim().is_empty() {
            return Ok(None);
        }
        // Propagated, never defaulted: a success whose body will not parse means
        // this CLI and that service disagree about the API, and reporting it as
        // "nothing came back" would send somebody looking at the database.
        Ok(Some(serde_json::from_str(&body).with_context(|| {
            format!("the service answered {status} with something this CLI could not read")
        })?))
    }

    async fn text(&self, req: reqwest::RequestBuilder) -> Result<String> {
        let res = req.send().await.context("reaching the tasks service")?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("{status}: {body}");
        }
        Ok(body)
    }

    fn writing(&self) -> Result<()> {
        if self.token.is_none() {
            bail!(
                "no token: set TASKS_TOKEN or write ~/.config/tasks/token. \
                 Reading works without one only when the service is unguarded."
            );
        }
        if self.session.is_none() {
            bail!(
                "no session id: pass --session or set TASKS_SESSION. \
                 The prompt hook prints this session's id."
            );
        }
        Ok(())
    }
}

/// Who a task is being handed to.
///
/// Parsed once, at the argument boundary, rather than matched as a string where
/// it is used: clap rejects nothing here — anything that is not one of the two
/// words is a session id — but having the type means the three destinations are
/// enumerated in one place and `assignee` cannot be handed a fourth spelling of
/// "nobody" that nothing recognises.
#[derive(Clone, Debug, PartialEq, Eq)]
enum To {
    /// Back in the pile, for whoever picks it up.
    Nobody,
    /// The person.
    ///
    /// ⚠ `me` means Pippijn even when a session types it. A session handing work
    /// back says "this one is for you", which is what the word means in the
    /// sentence being written; a session taking one on uses `start`, which is
    /// what it actually wants.
    Person,
    Session(String),
}

impl std::str::FromStr for To {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "nobody" | "none" | "" => To::Nobody,
            "me" | "pippijn" => To::Person,
            id => To::Session(id.to_string()),
        })
    }
}

/// The assignee the API takes.
fn assignee(to: &To) -> Value {
    match to {
        To::Nobody => json!({ "kind": "nobody" }),
        To::Person => json!({ "kind": "person", "id": "pippijn" }),
        To::Session(id) => json!({ "kind": "session", "id": id }),
    }
}

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

/// One task as one line — the same shape the digest injects, so what a session
/// reads in a list and what it is handed in a prompt cannot look like two
/// different systems.
fn line(task: &Value) -> String {
    let marker = match task["status"].as_str().unwrap_or("open") {
        "doing" => "- [>]",
        "done" => "- [x]",
        _ => "- [ ]",
    };
    let mut out = format!(
        "{marker} #{:<4} {}",
        task["id"].as_u64().unwrap_or(0),
        task["subject"].as_str().unwrap_or("")
    );
    if let Some(repo) = task["repo"].as_str() {
        out.push_str(&format!("  [{repo}]"));
    }
    let holder = &task["assignee"];
    if holder["kind"].as_str().unwrap_or("nobody") != "nobody" {
        let who = holder["name"]
            .as_str()
            .or_else(|| holder["id"].as_str())
            .unwrap_or("?");
        out.push_str(&format!("  ({who})"));
    }
    out
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = Client {
        http: reqwest::Client::builder()
            .build()
            .context("building the http client")?,
        base: cli
            .url
            .or_else(|| std::env::var("TASKS_URL").ok())
            .unwrap_or_else(|| DEFAULT_URL.to_string())
            .trim_end_matches('/')
            .to_string(),
        token: token(),
        session: cli
            .session
            .or_else(|| std::env::var("TASKS_SESSION").ok())
            .filter(|s| !s.trim().is_empty()),
    };

    match cli.command {
        Command::List { repo, mine, done } => {
            let mut query: Vec<(String, String)> = Vec::new();
            if let Some(repo) = repo {
                query.push(("repos".into(), repo));
            }
            if done {
                query.push(("done".into(), "true".into()));
            }
            if mine {
                let session = client
                    .session
                    .clone()
                    .context("--mine needs a session id (--session or $TASKS_SESSION)")?;
                query.push(("session".into(), session));
            }
            let req = client
                .request(reqwest::Method::GET, "/api/tasks")
                .query(&query);
            let tasks = client.send(req).await?.unwrap_or(json!([]));
            let tasks = tasks.as_array().cloned().unwrap_or_default();
            if tasks.is_empty() {
                println!("nothing open");
            }
            for task in &tasks {
                println!("{}", line(task));
            }
        }

        Command::Show { id } => {
            let req = client.request(reqwest::Method::GET, &format!("/api/tasks/{id}"));
            let task = client.send(req).await?.context("no such task")?;
            println!("{}", line(&task));
            // Only on `show`, never in a list: this is what somebody checking
            // whether #79 is *their* #79 needs, and it is dead weight on a line
            // that is scanned rather than read.
            if let Some(origin) = task["origin"].as_str() {
                println!("  was {origin}");
            }
            let body = task["body"].as_str().unwrap_or("").trim();
            if !body.is_empty() {
                println!("\n{body}");
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
        }

        Command::Add {
            subject,
            repo,
            body: raw,
            to,
        } => {
            client.writing()?;
            let mut payload = json!({ "subject": subject, "body": raw.as_deref().map(body).transpose()?.unwrap_or_default() });
            if let Some(repo) = repo {
                payload["repo"] = json!(repo);
            }
            if let Some(to) = &to {
                payload["assignee"] = assignee(to);
            }
            let req = client
                .request(reqwest::Method::POST, "/api/tasks")
                .json(&payload);
            let task = client.send(req).await?.context("no task came back")?;
            println!("{}", line(&task));
        }

        Command::Start { id } => patch(&client, id, json!({ "status": "doing" })).await?,
        Command::Done { id } => patch(&client, id, json!({ "status": "done" })).await?,
        Command::Move { id, to } => {
            patch(&client, id, json!({ "assignee": assignee(&to) })).await?
        }

        Command::Edit {
            id,
            subject,
            body: raw,
        } => {
            let mut change = json!({});
            if let Some(subject) = subject {
                change["subject"] = json!(subject);
            }
            if let Some(raw) = raw {
                change["body"] = json!(body(&raw)?);
            }
            if change.as_object().is_none_or(|o| o.is_empty()) {
                bail!("nothing to change: pass --subject or --body");
            }
            patch(&client, id, change).await?;
        }

        Command::Digest { repo } => {
            let mut query: Vec<(String, String)> = Vec::new();
            if let Some(repo) = repo {
                query.push(("repos".into(), repo));
            }
            let req = client
                .request(reqwest::Method::GET, "/api/digest")
                .query(&query);
            let text = client.text(req).await?;
            let bytes = text.len();
            println!("{text}");
            // The number is the point of running this by hand: it is the
            // per-turn cost of the whole system.
            eprintln!("\n({bytes} bytes)");
        }

        Command::Sessions => {
            let req = client.request(reqwest::Method::GET, "/api/sessions");
            let sessions = client.send(req).await?.unwrap_or(json!([]));
            for session in sessions.as_array().cloned().unwrap_or_default() {
                println!(
                    "{:<40} {:<24} {} open",
                    session["id"].as_str().unwrap_or(""),
                    session["name"].as_str().unwrap_or("—"),
                    session["open"].as_i64().unwrap_or(0)
                );
            }
        }

        Command::Rename { name } => {
            client.writing()?;
            let session = client.session.clone().expect("writing() checked it");
            let req = client
                .request(reqwest::Method::PATCH, &format!("/api/sessions/{session}"))
                .json(&json!({ "name": name }));
            client.send(req).await?;
            println!("{session} is now {name}");
        }
    }
    Ok(())
}

async fn patch(client: &Client, id: u64, change: Value) -> Result<()> {
    client.writing()?;
    let req = client
        .request(reqwest::Method::PATCH, &format!("/api/tasks/{id}"))
        .json(&change);
    let task = client.send(req).await?.context("no task came back")?;
    println!("{}", line(&task));
    Ok(())
}
