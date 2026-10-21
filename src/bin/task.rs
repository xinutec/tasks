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

/// The service, by its VPN name.
const DEFAULT_URL: &str = "https://tasks.xinutec.org";

/// What `task --help` adds: the levels come from [`Priority::gloss`], never a second copy.
fn long_about() -> String {
    let levels: String = Priority::all()
        .iter()
        .map(|p| format!("\n  {p}  {}", p.gloss()))
        .collect();
    format!(
        "The work Claude sessions and the user hand between each other.

Two facts decide how to use this, and neither is guessable from the commands:

  * A SESSION NEVER ENDS. Conversations go quiet and come back; there is no
    terminal state and nothing here goes stale because nobody is at the keyboard.
    Handing work to a conversation with no live process is QUEUEING it, not
    stranding it, and needs no apology.

  * A HOLDER'S OPEN TASKS ARE ITS FUTURE WORK, not what it is doing now. Thirty
    open against a session is a backlog addressed to that conversation, not
    thirty things in flight. `- [>]` is the mark for work actually in hand.

So the question to ask before handing something over is WHOSE SUBJECT IT IS, and
never who is online. Preferring whoever is awake would pile every task onto
whichever conversation happened to be running, which is the opposite of what a
list of addressed work is for.

PRIORITY is P0 to P4, and it is the one thing that reorders a list:{levels}

FILING ONE MEANS SAYING. `task add` takes `--priority`, or `--unassessed` for
work that is not yours to judge — filing into another session's domain is the
ordinary case. Both are answers and both sort at P2; the difference is that P2
claims somebody read it and called it ordinary, where UNASSESSED claims nobody
has. Leaving both off is not an answer and is refused.

THE PILE IS THE SECOND THING THAT MUST BE SAID. `--to nobody` needs
`--spare \"<why>\"` beside it, and a bare one is refused the same way a missing
priority is. `--to me` is the default and needs no argument; the pile is the one
holder that does, because filings to it were almost always corrected later. A
reason without `--to nobody` is refused too: it would say nothing true about a
held task.

UNASSESSED is not a sixth level — it sorts exactly where P2 does. So P0 and P1
rise above it and P3 and P4 sink below, and anything nobody has judged keeps its
place: oldest first, which is what makes old work get fixed rather than buried.

The escape is there so the required flag cannot be satisfied by typing P2 at a
question you did not answer. Everything on P0 and everything on P2 fail the same
way — the rank stops carrying information."
    )
}

/// What `task edit --help` adds, from [`density::RUBRIC`], so writers and the judge share
/// one standard.
fn edit_about() -> String {
    format!(
        "Change a task's words.

⚠ **Rewriting is the moment to put the conclusion back on top.** A body grows in
the order things happened, so what is still true sinks to the bottom — a verdict
can end up at the foot of a long ticket, or a plan in the last paragraph of a
ticket about something else. Lead with where it stands; the history goes under
it. Better still, put the state in `--subject`, which is the only part anybody
reads without opening the task.

⚠ **`--prepend` is how to do that in one command, and `--body` is not.** `--body`
replaces everything; recording an outcome with it deletes the filing unless the
old text is read out and pasted back first. That read gets skipped, and the
guard on `--body` only catches a body cut to a fraction of itself.

WHAT A BODY IS HELD TO. Once one has grown {sampler} characters since anybody
last rewrote it, a model reads it against these three rules and says what it
finds. It never refuses the write:

{rubric}

⚠ **Never write to \"as few words as possible\".** Told to compress, a model drops
the numbers and keeps the prose, because prose reads like the argument — and a
body is believed for its measurements. Density is the target and length is its
consequence, which is why rule 2 is one-sided: it cuts restatement and never
evidence.",
        sampler = density::SAMPLER,
        rubric = density::RUBRIC,
    )
}

#[derive(Parser)]
#[command(
    name = "task",
    about = "The work Claude sessions and the user hand between each other",
    long_about = long_about()
)]
struct Cli {
    /// Base URL of the service. Defaults to $TASKS_URL, then the VPN name.
    #[arg(long, global = true)]
    url: Option<String>,
    /// This conversation's session id. Defaults to $TASKS_SESSION, then $CLAUDE_CODE_SESSION_ID.
    #[arg(long, global = true)]
    session: Option<String>,
    /// Print the service's JSON, verbatim, instead of the human format.
    ///
    /// A list row has `id`, `subject`, `status` (`open`, `doing`, `done`, `dropped`)
    /// and `assignee`, an object `{kind, id, name}` (`kind`: `session`, `person`,
    /// `nobody`); there is no top-level `session` field. Unset optional fields like
    /// `priority` are absent, not null. `--pile`, `--mine` and `--to` answer the
    /// usual questions without filtering JSON.
    ///
    /// ⚠ `show --json` is bigger: it adds `body`, `body_html`, `events`,
    /// `restorable`. In a list, `detailed` is a boolean standing in for the prose;
    /// piped into `edit --body` it writes `True` over the body. `show <id> --body`
    /// prints the body alone.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// What is open: yours and the pile, unless asked otherwise.
    List {
        /// Every open task, whoever is holding it.
        #[arg(long)]
        all: bool,
        /// Strictly what this session holds, without the pile.
        #[arg(long, conflicts_with = "all")]
        mine: bool,
        /// Strictly the pile. Your prompt shows only the first few; the rest are here.
        #[arg(long, conflicts_with_all = ["all", "mine"])]
        pile: bool,
        /// What you filed that anyone else holds, the pile included: your prompt
        /// never shows another conversation's work, so this is how to see what you
        /// handed out.
        #[arg(long, alias = "handed", conflicts_with_all = ["all", "mine", "pile", "to"])]
        handed_out: bool,
        /// Include finished tasks.
        #[arg(long)]
        done: bool,
        /// What one holder carries: a session by name or id, `human`, or `nobody`.
        /// Strictly that holder. `--assignee` and `--holder` work too.
        #[arg(long, conflicts_with_all = ["all", "mine", "pile"], aliases = ["assignee", "holder"])]
        to: Option<To>,
    },
    /// Work on a few things and let the rest go quiet for a while.
    ///
    ///     task focus 849 850 --for 4h
    ///
    /// For four hours your prompt recites only those tasks and counts the rest.
    /// `task list` still shows everything, and P0 and overdue tasks break through.
    /// It always expires; past a day it would be a handover, which is `task move`.
    /// Naming no task shows the focus; `--clear` ends it.
    Focus {
        /// What you are working on; repeat for several. Replaces the set.
        ids: Vec<TaskRef>,
        /// How long: `4h`, `90m`, `2h30m`; a bare number is minutes. Required: expiry is
        /// what makes hiding safe.
        #[arg(long = "for")]
        period: Option<String>,
        /// End it now, before its hour.
        #[arg(long, conflicts_with_all = ["ids", "period"])]
        clear: bool,
    },
    /// One task, with its prose and its history.
    #[command(alias = "history")]
    Show {
        id: TaskRef,
        /// Print the body alone, exactly as stored, for diffing or editing a faithful copy.
        #[arg(long, conflicts_with = "json")]
        body: bool,
        /// The task before its latest edit. With `--body`: `task show 25 --previous --body
        /// > was.md`.
        #[arg(long)]
        previous: bool,
    },
    /// Put back the subject and body the latest edit replaced.
    ///
    /// Undo is itself an edit, recorded and undoable. ⚠ It reverts the task's last
    /// edit, not yours: if that was another conversation's it refuses, and
    /// `--anyway` insists. Check first with `task show <id> --previous`.
    Undo {
        id: TaskRef,
        /// Revert the last edit even though another conversation made it.
        #[arg(long)]
        anyway: bool,
    },
    /// File a task. Urgency is required: `--priority`, or `--unassessed`.
    #[command(group(clap::ArgGroup::new("rank").required(true).args(["priority", "unassessed"])))]
    Add {
        /// The one-line subject.
        // Optional only so a missing one is answered in a sentence, not by clap.
        subject: Option<String>,
        /// The body. `-` reads stdin, which avoids shell quoting.
        #[arg(long)]
        body: Option<String>,
        /// Who it is for: `me` (default: whoever is filing), `human`, `nobody`, or a session.
        #[arg(long)]
        to: Option<To>,
        /// Why this is nobody's; required by `--to nobody` and only by it. Pile filings
        /// were usually corrected later, so the pile is argued for.
        #[arg(long, value_name = "WHY")]
        spare: Option<String>,
        /// How urgent: P0 to P4, or say `--unassessed`. The levels, under
        /// `task --help`, are tests a task passes or fails; `P2` when none of the others
        /// fit.
        #[arg(long)]
        priority: Option<Priority>,
        /// Say you are not judging this one: when the work is not yours to rank, such as
        /// filing into another session's domain. Sorts like `P2`, but `P2` claims
        /// somebody read it and called it ordinary.
        #[arg(long, conflicts_with = "priority")]
        unassessed: bool,
        /// Task ids this one waits for. Repeat for several.
        #[arg(long = "blocked-on")]
        blocked_on: Vec<u64>,

        /// Task ids this one unblocks, recorded on them once this is filed. Like
        /// `--blocked-on`, the duplicate check doesn't compare against them: a blocker
        /// resembles what it unblocks.
        #[arg(long = "blocks")]
        blocks: Vec<u64>,
        /// The day it has to be done by: YYYY-MM-DD.
        #[arg(long)]
        due: Option<NaiveDate>,
        /// Refused by name: a task has had no repo since migration 0004 (the holder is
        /// the assignment). clap's `unexpected argument` would suggest a typo.
        #[arg(long, hide = true)]
        repo: Option<String>,
        /// The same.
        #[arg(long, hide = true)]
        project: Option<String>,
        /// Refused by name: the subject is the first argument, and `unexpected argument`
        /// reads as a quoting mistake.
        #[arg(long = "subject", hide = true)]
        subject_flag: Option<String>,
        /// File past a duplicate refusal you have just seen, naming each task
        /// it named that you judged different work: `--not-duplicate-of 2037,12`.
        ///
        /// Only after a refusal of the same subject in this conversation, and
        /// only naming every task it named; otherwise the check runs as usual.
        /// The filing's history records what it overruled. A check that cannot
        /// run files the task and says so.
        #[arg(long, value_delimiter = ',', num_args = 1..)]
        not_duplicate_of: Vec<u64>,
        /// Kept for older habits. The check runs anyway, and if it refuses, the
        /// refusal says which ids `--not-duplicate-of` must name.
        #[arg(long, hide = true)]
        no_duplicate_check: bool,
        #[arg(long, conflicts_with = "not_duplicate_of")]
        check_only: bool,
    },
    /// Mark a task as being worked on.
    Start { id: TaskRef },
    /// Mark a task finished. `close` is the same command.
    #[command(alias = "close")]
    Done {
        id: TaskRef,
        /// Who holds it after (otherwise the finisher, so lists say who did it).
        #[arg(long)]
        to: Option<To>,
        /// What came of it, written above the body before it closes, so no task closes
        /// without a word. `--reason` and `--message` work too; `-` reads stdin.
        #[arg(long, aliases = ["reason", "message"])]
        note: Option<String>,
    },
    /// Close a task without doing it: overtaken, obsolete, decided against. Nobody is credited.
    Drop {
        id: TaskRef,
        /// Why, written above the body first. A drop without a reason can't be told
        /// from a decision, and a model asked will invent one.
        #[arg(long, aliases = ["note", "message"])]
        reason: Option<String>,
    },
    /// Put a closed or started task back to open. The holder stays (reopening your
    /// own lands on your plate); `task move <id> nobody` sends it to the pile.
    Reopen { id: TaskRef },
    /// Hand a task over: `me`, `human`, `nobody`, or a session by name or id. A
    /// name matching nothing or two conversations is refused.
    ///
    /// Handing to a quiet conversation is queueing: sessions go offline and come
    /// back. Choose by whose subject it is, not who is at a keyboard. `nobody` is for
    /// work that suits whoever comes next, not the safe default; it costs every
    /// session's prompt.
    Move { id: TaskRef, to: To },
    /// Wait until a task is closed, then return.
    ///
    ///     task wait 1350
    ///
    /// ⚠ Run it in the background. Blocked on someone else's task, file the edge
    /// (`task edit <mine> --blocked-on <theirs>`), start this detached and go quiet:
    /// Claude Code wakes a session when a background command exits, and the digest
    /// can't, since a blocked session takes no turns.
    ///
    /// It ends when every named task is closed. Exit 0 means done; otherwise read
    /// stderr (a dropped blocker means the fix never happened). A restart loses the
    /// wait but not the edge.
    Wait {
        /// What to wait for: all of them, not the first.
        #[arg(required = true)]
        ids: Vec<TaskRef>,
        /// How long before giving up: `4h`, `90m`, `2h30m`; a bare number is minutes.
        /// Always bounded: a wait that hangs forever goes unnoticed.
        #[arg(long = "for", default_value = "24h")]
        period: String,
    },
    /// Change a task's words. `update` and `rank` are the same command.
    #[command(long_about = edit_about(), aliases = ["update", "rank"])]
    Edit {
        id: TaskRef,
        /// The one-line subject. `--title` is accepted too.
        #[arg(long, alias = "title")]
        subject: Option<String>,
        /// `-` reads stdin. Replaces the whole body; `--prepend` keeps it.
        #[arg(long, conflicts_with_all = ["prepend", "append"])]
        body: Option<String>,
        /// Put text above the body, keeping all of it. `-` reads stdin.
        ///
        ///     task edit 42 --prepend "DONE in a2c3ab6 — deployed and verified."
        ///
        /// Usually the one you want: where it stands first, history below.
        #[arg(long)]
        prepend: Option<String>,
        /// Put text below the body, keeping all of it: for what happened next rather than
        /// the conclusion. Combines with `--prepend`.
        #[arg(long)]
        append: Option<String>,
        /// Skip the model's read of a grown body. It never refuses, so this is for
        /// scripts, not for getting past it.
        #[arg(long = "no-density-check")]
        no_density_check: bool,
        /// Mean it, when `--body` would leave almost nothing of a long body: refused by
        /// default, since that is usually `detailed` from `--json` written over the prose.
        #[arg(long = "replace-body")]
        replace_body: bool,
        /// Rank it: P0 to P4 (`task --help`). There is no unrank: rank again to correct.
        #[arg(long)]
        priority: Option<Priority>,
        /// What this task waits for; repeat for several. Replaces the set (`--unblock`
        /// clears). It may not outrank a blocker, block itself or close a loop.
        #[arg(long = "blocked-on", conflicts_with = "unblock")]
        blocked_on: Vec<u64>,
        /// It is not waiting for anything any more.
        #[arg(long)]
        unblock: bool,
        /// The deadline, YYYY-MM-DD: evidence for a rank, until the last week, when it
        /// sorts as `P0!`. Earlier than an open blocker's is refused.
        #[arg(long, conflicts_with = "no_due")]
        due: Option<NaiveDate>,
        /// Take the deadline off.
        #[arg(long = "no-due")]
        no_due: bool,
    },
    /// Exactly what a prompt receives: for checking the cost.
    Digest,
    /// Who holds what, open/total: sessions, the person, the pile.
    Sessions {
        /// Every conversation ever, including those never given work: where to find a new one's id.
        #[arg(long)]
        all: bool,
    },
    /// Tell the service what this session now calls itself.
    Rename { name: String },
    /// What the duplicate check and the density read have been doing.
    Checks {
        /// How far back to look.
        #[arg(long, default_value_t = 7)]
        days: u32,
    },
    /// How long commands have taken, from real use (nothing polls, so a quiet day reads as quiet).
    Timings {
        /// How far back to look.
        #[arg(long, default_value_t = 7)]
        days: u32,
    },
}

/// The shared secret, from the environment or the Mac's file; never argv, which
/// is visible in process listings and transcripts. The CLI's only: the pod reads
/// `AGENT_TOKEN`, and must not hold the credential it verifies.
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

/// Which conversation this is: `$TASKS_SESSION` for scripts, else `$CLAUDE_CODE_SESSION_ID`.
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

/// What Claude Code calls this conversation now, from the transcript
/// ([`tasks::agent_name`]); silent on failure.
fn called_now(session: &str) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let projects = std::path::Path::new(&home).join(".claude").join("projects");
    tasks::agent_name::from_projects(&projects, session)
}

/// `Clone` so `main` can report the timing after the command; cheap, and
/// `reqwest::Client` shares its connection pool.
#[derive(Clone)]
struct Client {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
    session: Option<String>,
    /// What Claude Code calls this conversation, read once per command.
    called: Option<String>,
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
        // On every request, reads too, so a rename in Claude Code reaches the service by itself.
        if let Some(called) = &self.called {
            req = req.header(tasks::access::SESSION_NAME_HEADER, called);
        }
        req
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Option<Value>> {
        self.identified()?;
        // Built first so the method is visible: any non-GET changed the list, so the
        // hook's cached digest is dropped here, centrally, where no command can forget.
        let req = req.build().context("building the request")?;
        let wrote = req.method() != reqwest::Method::GET;
        let res = self
            .http
            .execute(req)
            .await
            .context("reaching the tasks service")?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        if !status.is_success() {
            // The service's message, which says what was wrong.
            let said = match serde_json::from_str::<Value>(&body) {
                Ok(parsed) => parsed["error"].as_str().map(str::to_string).unwrap_or(body),
                // Not JSON (an ingress page, a proxy timeout): the body is the message.
                Err(_) => body,
            };
            // 4xx is the service deciding, 5xx it failing; counted apart (`commands::Ended`).
            if status.is_client_error() {
                return Err(commands::declined(format!("{status}: {said}")));
            }
            bail!("{status}: {said}");
        }
        if wrote {
            self.forget_cached_digest();
        }
        if body.trim().is_empty() {
            return Ok(None);
        }
        // An unreadable success means CLI and service disagree about the API: an error,
        // never "nothing came back".
        Ok(Some(serde_json::from_str(&body).with_context(|| {
            format!("the service answered {status} with something this CLI could not read")
        })?))
    }

    async fn text(&self, req: reqwest::RequestBuilder) -> Result<String> {
        self.identified()?;
        let res = req.send().await.context("reaching the tasks service")?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("{status}: {body}");
        }
        Ok(body)
    }

    /// A typed holder as a session id ([`holder::resolve`]), refused with the known names
    /// when unresolved.
    async fn resolve(&self, to: To) -> Result<Target> {
        let typed = match to {
            To::Nobody => return Ok(Target::Nobody),
            To::Me => return Ok(Target::Me),
            To::Human => return self.human().await.map(Target::Person),
            To::Session(typed) => typed,
        };
        let typed = &typed;
        // Holders first: the short list, and the usual case.
        let mut known = known_sessions(
            &self
                .send(self.request(reqwest::Method::GET, "/api/holders"))
                .await?
                .unwrap_or(json!([])),
        );
        if !known.iter().any(|(id, name)| matches(id, name, typed)) {
            // The full session list only if that didn't answer.
            known = known_sessions(
                &self
                    .send(self.request(reqwest::Method::GET, "/api/sessions"))
                    .await?
                    .unwrap_or(json!([])),
            );
        }
        let pairs: Vec<(&str, Option<&str>)> = known
            .iter()
            .map(|(id, name)| (id.as_str(), name.as_deref()))
            .collect();
        match holder::resolve(pairs, typed) {
            Holder::Session(id) => Ok(Target::Session(id)),
            Holder::Unknown(names) => bail!(
                "no session called `{typed}`, and it is not an id this service knows. \
                 Assigning it anyway would hand the task to a conversation that is not \
                 there, which leaves every list but `--all`. Known: {}",
                names.join(", ")
            ),
            Holder::Ambiguous(ids) => bail!(
                "`{typed}` answers to {} conversations, as a name or an id, so this \
                 would guess: {}. Give the full id instead — `task sessions` prints both.",
                ids.len(),
                ids.join(", ")
            ),
        }
    }

    /// The person's account, as the service's holder list names it.
    async fn human(&self) -> Result<String> {
        let rows = self
            .send(self.request(reqwest::Method::GET, "/api/holders"))
            .await?
            .unwrap_or(json!([]));
        person_id(&rows).context("the service names no person to hand work to")
    }

    /// Drop the hook's cached digest; silent on failure.
    fn forget_cached_digest(&self) {
        let (Some(session), Ok(home)) = (&self.session, std::env::var("HOME")) else {
            return;
        };
        tasks::hook::forget_digest(std::path::Path::new(&home), session);
    }

    /// Refuse before the round trip when this CLI holds a token but no identity,
    /// a certain 401. Holding neither is the service's call. Called per request, not
    /// in `main`, so a command that sends nothing needs no identity. ⚠ Tests clear
    /// both variables: passing `--session` to get past this tests nothing.
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

    /// This conversation's id, what `me` resolves to, needed before building a request body.
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

/// Who a task is handed to, parsed once at the argument boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
enum To {
    /// Back in the pile, for whoever picks it up.
    Nobody,
    /// Whoever runs this. ⚠ For a session that is itself; the person is `human`.
    Me,
    /// The person; the service says which account.
    Human,
    /// A conversation by id or name, settled by [`Client::resolve`] before sending.
    Session(String),
}

impl std::str::FromStr for To {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "nobody" | "none" | "" => To::Nobody,
            "me" | "self" | "mine" => To::Me,
            "human" => To::Human,
            id => To::Session(id.to_string()),
        })
    }
}

/// A [`To`] with the person's account and session ids known.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Nobody,
    Me,
    Person(String),
    Session(String),
}

/// The assignee the API takes. `me` is resolved here: a body says what to
/// change, never who is changing it.
fn assignee(to: &Target, me: &str) -> Value {
    match to {
        Target::Nobody => json!({ "kind": "nobody" }),
        Target::Me => json!({ "kind": "session", "id": me }),
        Target::Person(id) => json!({ "kind": "person", "id": id }),
        Target::Session(id) => json!({ "kind": "session", "id": id }),
    }
}

/// Whether a row answers to what somebody typed, as an id or as a name.
fn matches(id: &str, name: &Option<String>, typed: &str) -> bool {
    id == typed || name.as_deref() == Some(typed)
}

/// The person row's id in `/api/holders`.
fn person_id(rows: &Value) -> Option<String> {
    rows.as_array()?
        .iter()
        .find(|row| row["kind"] == "person")?["id"]
        .as_str()
        .map(str::to_string)
}

/// One row of `/api/holders` or `/api/sessions`, reduced to what naming needs.
fn known_sessions(rows: &Value) -> Vec<(String, Option<String>)> {
    rows.as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|row| row["kind"] != "person" && row["kind"] != "nobody")
        .filter_map(|row| {
            let id = row["id"].as_str()?.to_string();
            Some((id, row["name"].as_str().map(str::to_string)))
        })
        .collect()
}

/// The model for checks: the cheapest, since it shares the session's allowance.
/// Named in full, so changing it is a change to this line.
const CHECKER: &str = "claude-haiku-4-5-20251001";

/// How long one check may take. Outside the spread, not at its middle: run time
/// varies severalfold with deliberation, and only the tail pays for the margin.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(120);

/// How much a check may deliberate. Unbounded, it costs several times longer for
/// the same findings; zero returns a fast false `DENSE` on the most tangled
/// bodies. The filing check uses it too.
const DELIBERATION: &str = "1024";

/// The bound for reading a body. Deliberation, not size, makes a read slow. Wider
/// than [`PATIENCE`] because it runs after the write; not much, because a run
/// that hits it produced nothing.
const READING: std::time::Duration = std::time::Duration::from_secs(90);

/// What this conversation's latest refusal of `subject` named, or `None` when
/// nothing has refused it.
async fn licence(client: &Client, subject: &str) -> Result<Option<Vec<u64>>> {
    let req = client
        .request(reqwest::Method::GET, "/api/checks/licence")
        .query(&[("subject", subject)]);
    let got = client
        .send(req)
        .await?
        .context("no licence answer came back")?;
    let said: checks::Licence =
        serde_json::from_value(got).context("reading whether a refusal licenses this")?;
    Ok(said.matched)
}

/// Every open task as `(id, subject)`, all holders (the duplicate that matters
/// is another conversation's), and the ids this session holds. Read before the
/// POST, so both checks refuse while there is nothing to undo.
async fn open_now(client: &Client) -> Result<(Vec<(u64, String)>, Vec<u64>)> {
    let query = list_query(true, false, false, false, false, None, None)?;
    let req = client
        .request(reqwest::Method::GET, "/api/tasks")
        .query(&query);
    let open = client.send(req).await?.unwrap_or(json!([]));
    let rows = open.as_array().map(Vec::as_slice).unwrap_or_default();
    let me = client.session.as_deref();
    let mine = rows
        .iter()
        .filter(|task| {
            task["assignee"]["kind"] == "session"
                && me.is_some_and(|me| task["assignee"]["id"] == me)
        })
        .filter_map(|task| task["id"].as_u64())
        .collect();
    let corpus = rows
        .iter()
        .filter_map(|task| Some((task["id"].as_u64()?, task["subject"].as_str()?.to_string())))
        .collect();
    Ok((corpus, mine))
}

/// Every closed task worth reading, as the cached block. A failure costs no
/// filing (empty list); the skipped count is reported ([`duplicates::worth_reading`]).
async fn settled_now(client: &Client) -> (Vec<duplicates::Settled>, usize) {
    let Ok(query) = list_query(true, false, false, false, true, None, None) else {
        return (Vec::new(), 0);
    };
    let req = client
        .request(reqwest::Method::GET, "/api/tasks")
        .query(&query);
    let Ok(Some(all)) = client.send(req).await else {
        return (Vec::new(), 0);
    };
    let rows = all.as_array().map(Vec::as_slice).unwrap_or_default();
    let mut kept = Vec::new();
    let mut skipped = 0usize;
    for task in rows {
        let Some(status) = task["status"]
            .as_str()
            .and_then(|s| s.parse::<Status>().ok())
        else {
            continue;
        };
        if status.is_open() {
            continue;
        }
        let (Some(id), Some(subject)) = (task["id"].as_u64(), task["subject"].as_str()) else {
            continue;
        };
        if !duplicates::worth_reading(task["detailed"].as_bool().unwrap_or(false)) {
            skipped += 1;
            continue;
        }
        kept.push(duplicates::Settled {
            id,
            subject: subject.to_string(),
            dropped: status == Status::Dropped,
        });
    }
    (kept, skipped)
}

/// What a model names, open or closed, as already saying what is being filed.
async fn already_filed(
    client: &Client,
    corpus: &[(u64, String)],
    settled: &[duplicates::Settled],
    subject: &str,
) -> Result<Vec<duplicates::Match>> {
    if corpus.is_empty() && settled.is_empty() {
        return Ok(Vec::new());
    }
    let asked = duplicates::prompt(subject, corpus, !settled.is_empty());
    // The closed list isn't counted: it is the same cached bytes every filing.
    let input_chars = asked.chars().count().min(u32::MAX as usize) as u32;
    let prefix = (!settled.is_empty()).then(|| duplicates::settled_block(settled));
    let (said, elapsed_ms) = ask_with(&asked, prefix.as_deref(), PATIENCE).await;
    // Against both lists; `split` decides which this filing is told.
    let known: Vec<(u64, String)> = corpus
        .iter()
        .cloned()
        .chain(settled.iter().map(|t| (t.id, t.subject.clone())))
        .collect();
    let found = match &said {
        Ok(words) => duplicates::parse(words, &known),
        Err(_) => Vec::new(),
    };
    // Only when it named something: the key licenses an override.
    let refused = (!found.is_empty()).then(|| checks::subject_key(subject));
    recorded(
        client,
        checks::Run {
            kind: checks::Kind::Filing,
            task_id: None,
            input_chars,
            accreted: None,
            elapsed_ms,
            outcome: checks::outcome(&said, !found.is_empty()),
            subject_key: refused,
            matched: found.iter().map(|one| one.id).collect(),
            // A filing check keeps no words: there is no task yet.
            said: None,
        },
    )
    .await;
    // The caller prints the failure: a table row doesn't tell the session.
    said?;
    Ok(found)
}

/// Report one run; silent on failure, since the call is over.
async fn recorded(client: &Client, run: checks::Run) {
    let req = client
        .request(reqwest::Method::POST, "/api/checks")
        .json(&run);
    let _ = client.send(req).await;
}

/// Whether this process waited for a model, set where the waiting happens so
/// timeouts count.
static WAITED_FOR_A_MODEL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Ask a one-shot session and leave nothing behind: each call leaves a
/// transcript, deleted as soon as the answer is in, failures included. `prefix`
/// is the cached part and must not vary ([`duplicates::settled_block`]); it goes
/// via a file, like the prompt ([`call`]).
async fn ask_with(
    prompt: &str,
    prefix: Option<&str>,
    patience: std::time::Duration,
) -> (Result<String>, u32) {
    WAITED_FOR_A_MODEL.store(true, std::sync::atomic::Ordering::Relaxed);
    let named = named();
    let carried = prefix.and_then(|text| {
        let path = std::env::temp_dir().join(format!("task-settled-{named}.txt"));
        std::fs::write(&path, text).ok().map(|()| path)
    });
    let started = std::time::Instant::now();
    let said = call(prompt, &named, carried.as_deref(), patience).await;
    // Before `discard`, which isn't part of what is measured.
    let took = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    discard(&named);
    if let Some(path) = carried {
        let _ = std::fs::remove_file(path);
    }
    (said, took)
}

/// The call itself. The prompt goes on stdin: it carries every open title, too
/// big for an argument and visible in process listings.
async fn call(
    prompt: &str,
    named: &str,
    prefix: Option<&std::path::Path>,
    patience: std::time::Duration,
) -> Result<String> {
    let mut command = tokio::process::Command::new("claude");
    command
        .current_dir(std::env::temp_dir())
        .arg("-p")
        .args(["--session-id", named])
        .args(["--model", CHECKER]);
    if let Some(path) = prefix {
        command.arg("--append-system-prompt-file").arg(path);
    }
    let mut child = command
        // The setting that decides a check's cost ([`DELIBERATION`]).
        .env("MAX_THINKING_TOKENS", DELIBERATION)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("no `claude` on PATH")?;
    let mut stdin = child.stdin.take().context("claude took no stdin")?;
    stdin
        .write_all(prompt.as_bytes())
        .await
        .context("writing the prompt")?;
    stdin.flush().await.context("writing the prompt")?;
    // Closed: `-p` reads to end of file.
    drop(stdin);
    let out = tokio::time::timeout(patience, child.wait_with_output())
        .await
        .with_context(|| format!("no answer in {}s", patience.as_secs()))?
        .context("waiting for claude")?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A v4 UUID for one of these calls, by hand: `rand` is already here.
fn named() -> String {
    let mut bytes = [0u8; 16];
    rand::fill(&mut bytes);
    // Version 4, variant 1: the CLI validates it.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Remove the call's transcript; nothing to remove is not a failure.
fn discard(named: &str) {
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    let projects = std::path::Path::new(&home).join(".claude").join("projects");
    if let Some(path) = tasks::agent_name::transcript_of(&projects, named) {
        let _ = std::fs::remove_file(path);
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

/// One task as one line, the digest's shape, so a list and a prompt look like
/// one system. Unlike the digest, a pile row says who filed it.
fn line(task: &Task) -> String {
    let marker = task.status.marker();
    // Rank first, padded so lines align; `!` marks a rank a deadline raised.
    let rank = match task.escalated_to {
        Some(raised) => format!("{}!", raised.as_str()),
        None => task
            .priority
            .map(|p| p.as_str().to_string())
            .unwrap_or_default(),
    };
    let mut out = format!("{marker} #{:<4} {rank:<3} {}", task.id, task.subject);
    if let Some(due) = task.due {
        if task.overdue {
            out.push_str(&format!("  OVERDUE {due}"));
        } else {
            out.push_str(&format!("  due {due}"));
        }
    }
    if task.blocked && !task.blocked_on.is_empty() {
        let ids: Vec<String> = task.blocked_on.iter().map(|v| format!("#{v}")).collect();
        out.push_str(&format!("  ⛔{}", ids.join(",")));
    }
    // On `task show`'s first line, so `head` still gets the count.
    match task.body_lines {
        0 => {}
        1 => out.push_str("  [1 line]"),
        n => out.push_str(&format!("  [{n} lines]")),
    }
    // The digest's marker, from the same field.
    if let Some(chars) = task.sprawl_chars {
        out.push_str(&format!("  [sprawl {}]", tasks::digest::thousands(chars)));
    }
    // `label()` is name, else id.
    if task.assignee.kind != AssigneeKind::Nobody {
        out.push_str(&format!("  ({})", task.assignee.label()));
    } else if let Some(from) = &task.filed_by {
        out.push_str(&format!("  (from {from})"));
    }
    out
}

/// Print an answer: verbatim with `--json`, else drawn. One helper, so no command ignores the flag.
fn emit(json: bool, value: &Value, human: impl FnOnce()) {
    if json {
        // Can't fail on a parsed Value; compact is still correct if it did.
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        );
    } else {
        human();
    }
}

impl Command {
    /// The name a command is recorded under. ⚠ Exhaustive: a catch-all would merge two
    /// commands' histories.
    fn verb(&self) -> &'static str {
        match self {
            Command::List { .. } => "list",
            Command::Focus { .. } => "focus",
            Command::Show { .. } => "show",
            Command::Undo { .. } => "undo",
            Command::Add { .. } => "add",
            Command::Start { .. } => "start",
            Command::Done { .. } => "done",
            Command::Drop { .. } => "drop",
            Command::Reopen { .. } => "reopen",
            Command::Move { .. } => "move",
            Command::Wait { .. } => "wait",
            Command::Edit { .. } => "edit",
            Command::Digest => "digest",
            Command::Sessions { .. } => "sessions",
            Command::Rename { .. } => "rename",
            Command::Checks { .. } => "checks",
            Command::Timings { .. } => "timings",
        }
    }
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
            let skip = match (
                not_duplicate_of.is_empty() && !no_duplicate_check,
                check_only,
            ) {
                (true, _) | (_, true) => false,
                (false, false) => match licence(&client, &subject).await {
                    Ok(Some(named)) => {
                        let missing: Vec<u64> = named
                            .into_iter()
                            .filter(|id| !not_duplicate_of.contains(id))
                            .collect();
                        if !missing.is_empty() {
                            return Err(commands::declined(checks::unnamed(&missing)));
                        }
                        true
                    }
                    Ok(None) => {
                        eprintln!(
                            "(nothing has refused this subject yet, so the duplicate check runs)"
                        );
                        false
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

/// The focus a route answered with; `null` is ordinary.
fn parse_focus(value: &Value) -> Option<tasks::tasks::focus::Focus> {
    serde_json::from_value(value.clone()).ok()
}

/// A focus as one line, with a countdown: fine here, never in the cached digest.
fn describe(focus: &tasks::tasks::focus::Focus) -> String {
    let ids: Vec<String> = focus.tasks.iter().map(|id| format!("#{id}")).collect();
    format!(
        "focused on {} until {} UTC — {} left. P0 and overdue still break through; \
         `task list` still shows everything.",
        ids.join(" "),
        focus.until.format("%H:%M"),
        tasks::tasks::focus::spell(focus.until - chrono::Utc::now()),
    )
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

/// A model's read of a grown body; every failure is silence, since the edit landed.
async fn accreting(client: &Client, id: TaskRef, updated: &Value) {
    let Some(accreted) = updated["replaced"]["accreted"].as_u64() else {
        return;
    };
    if !density::worth_asking(accreted as usize) {
        return;
    }
    // Re-read: additions carry only fragments, and the whole document is judged.
    let req = client.request(reqwest::Method::GET, &id.path());
    let Ok(Some(task)) = client.send(req).await else {
        return;
    };
    let Some(body) = task["body"].as_str() else {
        return;
    };
    let asked = density::prompt(id.id(), accreted as usize, body);
    let input_chars = asked.chars().count().min(u32::MAX as usize) as u32;
    let (said, elapsed_ms) = ask_with(&asked, None, READING).await;
    let advice = said
        .as_ref()
        .ok()
        .and_then(|words| density::advice(words, id.id()));
    recorded(
        client,
        checks::Run {
            kind: checks::Kind::Density,
            task_id: Some(id.id()),
            input_chars,
            accreted: Some(accreted.min(u64::from(u32::MAX)) as u32),
            elapsed_ms,
            outcome: checks::outcome(&said, advice.is_some()),
            subject_key: None,
            matched: Vec::new(),
            // What the model said, not this command's wrapper.
            said: advice
                .is_some()
                .then(|| said.as_ref().ok().map(|words| words.trim().to_string()))
                .flatten(),
        },
    )
    .await;
    if let Some(advice) = advice {
        eprintln!("{advice}");
    }
}

/// What an edit landed on, told with its undo ([`Replaced`](tasks::tasks::types::Replaced)).
fn displaced(task: &Value, id: u64) -> Option<String> {
    let was = task.get("replaced")?;
    let (before, after) = (was["was"].as_u64()?, was["now"].as_u64()?);
    Some(format!(
        "replaced text last written {} by {} ({before} → {after} chars) — task undo {} puts it back",
        was["at"].as_str().unwrap_or(""),
        was["by"].as_str().unwrap_or(""),
        id
    ))
}

/// One kind of check per line. A timeout always prints: quiet and timeout both end in silence.
fn tallied(line: &checks::Tally) -> String {
    let kind = match line.kind {
        checks::Kind::Filing => "filing",
        checks::Kind::Density => "density",
        checks::Kind::Collision => "collision",
    };
    let outcomes: Vec<String> = [
        ("quiet", line.quiet),
        ("spoke", line.spoke),
        ("timeout", line.timeout),
        ("error", line.error),
    ]
    .into_iter()
    .filter(|(_, n)| *n > 0)
    .map(|(what, n)| format!("{n} {what}"))
    .collect();
    format!(
        "{kind:8} {:3} runs · {} · {} median, {} p90, {} worst",
        line.runs,
        outcomes.join(", "),
        spell(line.median_ms),
        spell(line.p90_ms),
        spell(line.worst_ms),
    )
}

/// One command's line; the run count first, as its weight.
fn timed_line(line: &commands::Tally) -> String {
    let failed = if line.failed > 0 {
        format!(", {} failed", line.failed)
    } else {
        String::new()
    };
    format!(
        "{:10} {:4} runs{failed} · {} median, {} p90, {} worst",
        line.verb,
        line.runs,
        spell(line.median_ms),
        spell(line.p90_ms),
        spell(line.worst_ms),
    )
}

/// Milliseconds as somebody would say them.
fn spell(ms: u32) -> String {
    let seconds = f64::from(ms) / 1000.0;
    if seconds < 10.0 {
        format!("{seconds:.1}s")
    } else {
        format!("{}s", seconds.round() as u32)
    }
}
