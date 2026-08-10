//! `task` — the CLI half of this service, and the half a Claude session uses.
//!
//! **It mirrors the app, and that is a rule rather than a convenience.** Pippijn
//! reads the list on a phone; a session has no browser at all. If the two
//! surfaces diverge, one party is working from a picture of the work the other
//! cannot see — which is the exact failure this service exists to prevent.
//! Anything that becomes visible in the UI gets a line here.
//!
//! **Who am I?** A session is identified by the CLI's own session id, and it
//! does not have to be told: Claude Code puts it in `$CLAUDE_CODE_SESSION_ID`
//! in every shell it runs, so `task list` works with nothing set up.
//! `--session` and `$TASKS_SESSION` override it, in that order, for a script
//! acting on some other conversation's behalf.
//!
//! ⚠ **There is no anonymous mode, for reads either.** The service refuses a
//! request that does not say which conversation it is (`access.rs`), because
//! the actor is derived from the credential and a change filed against nobody
//! is the one thing the history must not contain. This CLI stops before the
//! round trip and says which of the two halves — token, identity — is missing.
//!
//! **Naming a task.** Every command that takes one accepts `79` or `#79` as the
//! digest prints it. The `recall#79` spelling — a task by what a session called
//! it before the migration — went with the columns behind it in
//! `migrations/0003_drop_origin.sql`, once every reference that needed it had
//! been rewritten to a live id.
//!
//! ```text
//! task list [--all|--mine] [--done]       yours and the pile; or wider, or narrower
//! task show <id>                            one task, its prose and its history
//! task add <subject> [--body -] [--to me|pippijn|<session>|nobody]
//! task start <id> / task done <id> [--to W] move it along
//! task drop <id>                            close it without doing it
//! task move <id> me|pippijn|<session>|nobody  hand it over
//! task edit <id> [--subject S] [--body -]   change the words
//! task digest                              exactly what a prompt receives
//! task rename <name>                        tell the service what I call myself
//! ```

use std::io::Read;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};

use tasks::tasks::reference::TaskRef;
use tasks::tasks::selection::list_query;

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
    /// This conversation's CLI session id. Defaults to $TASKS_SESSION, then to
    /// $CLAUDE_CODE_SESSION_ID, which Claude Code already sets.
    #[arg(long, global = true)]
    session: Option<String>,
    /// Print what the service answered, verbatim, instead of the human format.
    ///
    /// ⚠ **The service's JSON, reprinted — not rebuilt here.** A second
    /// serialisation in this binary would be a second shape to keep level with
    /// the API by hand, and the whole value of the flag is that a script can
    /// rely on the documented one. It is why the human format is free to change.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// What is open — yours and the pile, unless asked otherwise.
    List {
        /// Every open task, whoever is holding it.
        #[arg(long)]
        all: bool,
        /// Strictly what this session holds, without the pile.
        #[arg(long, conflicts_with = "all")]
        mine: bool,
        /// Include finished tasks.
        #[arg(long)]
        done: bool,
    },
    /// One task, with its prose and its history.
    Show {
        id: TaskRef,
        /// Print the body alone, with no header and no history — for diffing
        /// prose against another copy of it, which is what checking a migration
        /// consists of.
        #[arg(long, conflicts_with = "json")]
        body: bool,
    },
    /// File a task.
    Add {
        subject: String,
        /// The body. `-` reads stdin, which is how a session writes a long one
        /// without fighting shell quoting.
        #[arg(long)]
        body: Option<String>,
        /// Who it is for: `me` (the default — whoever is filing), `pippijn`,
        /// `nobody` for the pile, or a session id.
        #[arg(long)]
        to: Option<To>,
    },
    /// Mark a task as being worked on.
    Start { id: TaskRef },
    /// Mark a task finished.
    Done {
        id: TaskRef,
        /// Where it goes instead. Finishing a task makes the finisher its
        /// holder, so that every later list says who did it; this is how to
        /// close one and hand it on in the same breath.
        #[arg(long)]
        to: Option<To>,
    },
    /// Close a task WITHOUT doing it: overtaken, obsolete, decided against.
    ///
    /// The counterpart to `done`, and the reason both exist: a task that has
    /// gone out of date has to be able to leave the list without anybody being
    /// credited with having done it. If why it went matters, write it — `task
    /// edit <id> --body -` — because that is prose and there is no field for it.
    Drop { id: TaskRef },
    /// Hand a task over: `me` (this conversation), `pippijn`, `nobody`, or a
    /// session id.
    Move { id: TaskRef, to: To },
    /// Change a task's words.
    Edit {
        id: TaskRef,
        #[arg(long)]
        subject: Option<String>,
        /// `-` reads stdin.
        #[arg(long)]
        body: Option<String>,
    },
    /// Exactly what a prompt receives — for checking the cost, not for reading.
    Digest,
    /// Who holds what: every session, Pippijn, and the pile — open/total each.
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

/// Which conversation this is, when it was not passed on the command line.
///
/// `$CLAUDE_CODE_SESSION_ID` is set in every shell Claude Code runs, which is
/// why there is nothing to configure: a session cannot forget to say who it is,
/// and — more to the point — cannot mistype *another* conversation's id into its
/// own history. `$TASKS_SESSION` still wins, for a script standing in for one.
fn session_id() -> Option<String> {
    ["TASKS_SESSION", "CLAUDE_CODE_SESSION_ID"]
        .into_iter()
        .find_map(|name| {
            std::env::var(name)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        })
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

    /// Refuse before the round trip when this CLI holds half a credential.
    ///
    /// ⚠ Only that one shape. A token with nobody behind it cannot be answered
    /// by *any* deployment — the service needs both halves to file a change
    /// against somebody, for reads as well, so sending it is a guaranteed 401
    /// whose message would be about the service rather than about this machine.
    /// Holding **neither** is left to the service, which is the only thing that
    /// knows whether it is guarded: a dev server with no `AGENT_TOKEN` answers
    /// everybody as the owner, and refusing here would break that loop.
    fn identified(&self) -> Result<()> {
        if self.token.is_some() && self.session.is_none() {
            bail!(
                "a token but no session id: this conversation is not saying who it is. \
                 Claude Code normally sets $CLAUDE_CODE_SESSION_ID; outside it, \
                 pass --session or set $TASKS_SESSION."
            );
        }
        Ok(())
    }

    /// This conversation's own id, which is what `me` resolves to.
    ///
    /// Separate from [`writing`](Self::writing) with the same message because a
    /// destination is worked out before the request that would have complained:
    /// `task move 5 me` has to know who "me" is in order to build the body.
    fn me(&self) -> Result<&str> {
        self.session.as_deref().context(
            "no session id: pass --session or set TASKS_SESSION. \
             Claude Code normally sets $CLAUDE_CODE_SESSION_ID.",
        )
    }

    fn writing(&self) -> Result<()> {
        if self.token.is_none() {
            bail!(
                "no token: set TASKS_TOKEN or write ~/.config/tasks/token. \
                 Writing is never anonymous, so there is no unguarded case here."
            );
        }
        if self.session.is_none() {
            bail!(
                "no session id: pass --session or set TASKS_SESSION. \
                 Claude Code normally sets $CLAUDE_CODE_SESSION_ID."
            );
        }
        Ok(())
    }
}

/// Who a task is being handed to.
///
/// Parsed once, at the argument boundary, rather than matched as a string where
/// it is used: clap rejects nothing here — anything that is not one of the three
/// words is a session id — but having the type means the destinations are
/// enumerated in one place and `assignee` cannot be handed a fourth spelling of
/// "nobody" that nothing recognises.
#[derive(Clone, Debug, PartialEq, Eq)]
enum To {
    /// Back in the pile, for whoever picks it up.
    Nobody,
    /// Whoever is running this — for a session, itself.
    ///
    /// ⚠ **`me` used to mean Pippijn even when a session typed it**, on the
    /// argument that a session saying "me" was writing "this one is for you".
    /// It read the sentence right and the situation wrong: nothing was ever
    /// implicitly a session's own, so the word every conversation reached for
    /// handed its work to the person. Pippijn's rule is that a Claude session
    /// dealing with a task should own it by default, as the built-in task tool
    /// does. Handing work to the person is `pippijn`, which says so.
    Me,
    /// The person, by name.
    Person,
    Session(String),
}

impl std::str::FromStr for To {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "nobody" | "none" | "" => To::Nobody,
            "me" | "self" | "mine" => To::Me,
            "pippijn" => To::Person,
            id => To::Session(id.to_string()),
        })
    }
}

/// The assignee the API takes.
///
/// `me` is resolved here rather than on the far side: the session id is
/// something this process knows and the service must not take on faith — a
/// request body says *what* to change and never *who* is changing it, so there
/// is no wire spelling of "whoever is asking" for a caller to claim.
fn assignee(to: &To, me: &str) -> Value {
    match to {
        To::Nobody => json!({ "kind": "nobody" }),
        To::Me => json!({ "kind": "session", "id": me }),
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
///
/// ⚠ **One deliberate difference, and it is the only one: a pile row says who
/// filed it.** The digest stays silent there and must — it is what every
/// session pays for on every turn, and `src/digest.rs` refuses a column of
/// holders for exactly this reason. A list is fetched when somebody has just
/// asked what to pick up, and that is the moment the answer is worth its bytes.
/// So: seeing the pile stays free, and deciding costs one command rather than
/// opening a task (548 bytes against 2,732, measured on #19).
fn line(task: &Value) -> String {
    let marker = match task["status"].as_str().unwrap_or("open") {
        "doing" => "- [>]",
        "done" => "- [x]",
        "dropped" => "- [-]",
        _ => "- [ ]",
    };
    let mut out = format!(
        "{marker} #{:<4} {}",
        task["id"].as_u64().unwrap_or(0),
        task["subject"].as_str().unwrap_or("")
    );
    let holder = &task["assignee"];
    if holder["kind"].as_str().unwrap_or("nobody") != "nobody" {
        let who = holder["name"]
            .as_str()
            .or_else(|| holder["id"].as_str())
            .unwrap_or("?");
        out.push_str(&format!("  ({who})"));
    } else if let Some(from) = task["filed_by"].as_str() {
        out.push_str(&format!("  (from {from})"));
    }
    out
}

/// Print a service answer: verbatim when `--json` was asked for, otherwise
/// however the caller draws it.
///
/// One helper rather than a check at each call site, so a command cannot be
/// added that quietly ignores the flag — which would be worse than not having
/// it, because a script would parse the human format believing it was JSON.
fn emit(json: bool, value: &Value, human: impl FnOnce()) {
    if json {
        // `to_string_pretty` on an already-parsed Value cannot fail; the compact
        // form is a correct answer rather than a mask if it somehow does.
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        );
    } else {
        human();
    }
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
        session: cli.session.or_else(session_id),
    };
    client.identified()?;

    match cli.command {
        Command::List { all, mine, done } => {
            let query = list_query(all, mine, done, client.session.as_deref())?;
            let req = client
                .request(reqwest::Method::GET, "/api/tasks")
                .query(&query);
            let tasks = client.send(req).await?.unwrap_or(json!([]));
            emit(cli.json, &tasks, || {
                let tasks = tasks.as_array().cloned().unwrap_or_default();
                if tasks.is_empty() {
                    println!("nothing open");
                }
                for task in &tasks {
                    println!("{}", line(task));
                }
            });
        }

        Command::Show {
            id,
            body: only_body,
        } => {
            let req = client.request(reqwest::Method::GET, &id.path());
            let task = client.send(req).await?.context("no such task")?;
            if only_body {
                // Exactly the stored markdown and nothing else — no trailing
                // newline added or removed — so it can be diffed against
                // another copy without the diff being about this program.
                print!("{}", task["body"].as_str().unwrap_or_default());
                return Ok(());
            }
            emit(cli.json, &task, || {
                println!("{}", line(&task));
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
            });
        }

        Command::Add {
            subject,
            body: raw,
            to,
        } => {
            client.writing()?;
            let mut payload = json!({ "subject": subject, "body": raw.as_deref().map(body).transpose()?.unwrap_or_default() });
            if let Some(to) = &to {
                payload["assignee"] = assignee(to, client.me()?);
            }
            let req = client
                .request(reqwest::Method::POST, "/api/tasks")
                .json(&payload);
            let task = client.send(req).await?.context("no task came back")?;
            emit(cli.json, &task, || println!("{}", line(&task)));
        }

        Command::Start { id } => patch(&client, cli.json, id, json!({ "status": "doing" })).await?,
        Command::Done { id, to } => {
            let mut change = json!({ "status": "done" });
            if let Some(to) = &to {
                change["assignee"] = assignee(to, client.me()?);
            }
            patch(&client, cli.json, id, change).await?
        }
        Command::Drop { id } => {
            patch(&client, cli.json, id, json!({ "status": "dropped" })).await?
        }
        Command::Move { id, to } => {
            patch(
                &client,
                cli.json,
                id,
                json!({ "assignee": assignee(&to, client.me()?) }),
            )
            .await?
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
            patch(&client, cli.json, id, change).await?;
        }

        Command::Digest => {
            let query: Vec<(String, String)> = Vec::new();
            let req = client
                .request(reqwest::Method::GET, "/api/digest")
                .query(&query);
            if cli.json {
                // Refused rather than ignored: `digest` is the one endpoint
                // that answers in text/plain, and deliberately — its consumer
                // is a hook whose whole contract is to print it. Serialising it
                // here would invent a shape the service does not have.
                bail!(
                    "digest is plain text by design — it is exactly what a prompt \
                     receives. `task list --json` is the machine-readable list."
                );
            }
            let text = client.text(req).await?;
            let bytes = text.len();
            println!("{text}");
            // The number is the point of running this by hand: it is the
            // per-turn cost of the whole system.
            eprintln!("\n({bytes} bytes)");
        }

        Command::Sessions => {
            let req = client.request(reqwest::Method::GET, "/api/holders");
            let holders = client.send(req).await?.unwrap_or(json!([]));
            emit(cli.json, &holders, || {
                for holder in holders.as_array().cloned().unwrap_or_default() {
                    // `open/total`, not `open`: a bare 0 reads as an idle session,
                    // and `0/56` is one that has cleared its plate. The id is the
                    // handle for `task move`, so it stays in the line even though
                    // the name is what is read.
                    println!(
                        "{:<40} {:<24} {:>3}/{:<4} open",
                        holder["id"].as_str().unwrap_or(""),
                        holder["name"].as_str().unwrap_or("—"),
                        holder["open"].as_i64().unwrap_or(0),
                        holder["total"].as_i64().unwrap_or(0),
                    );
                }
            });
        }

        Command::Rename { name } => {
            client.writing()?;
            let session = client.session.clone().expect("writing() checked it");
            let req = client
                .request(reqwest::Method::PATCH, &format!("/api/sessions/{session}"))
                .json(&json!({ "name": name }));
            let answer = client.send(req).await?.unwrap_or(json!({}));
            emit(cli.json, &answer, || println!("{session} is now {name}"));
        }
    }
    Ok(())
}

/// Change a task, named either way.
async fn patch(client: &Client, json: bool, id: TaskRef, change: Value) -> Result<()> {
    client.writing()?;
    let id = id.id();
    let req = client
        .request(reqwest::Method::PATCH, &format!("/api/tasks/{id}"))
        .json(&change);
    let task = client.send(req).await?.context("no task came back")?;
    emit(json, &task, || println!("{}", line(&task)));
    Ok(())
}
