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
//! task list [--all|--mine|--pile] [--done] yours and the pile; wider; narrower; spare
//! task show <id> [--previous]               one task, its prose and its history
//! task undo <id>                            put back what the last edit replaced
//! task add <subject> [--body -] [--to me|pippijn|<session>|nobody] [--priority P3]
//! task start <id> / task done <id> [--to W] move it along
//! task drop <id>                            close it without doing it
//! task reopen <id>                          put it back to open
//! task move <id> me|pippijn|<session>|nobody  hand it over
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
use tasks::tasks::density;
use tasks::tasks::duplicates;
use tasks::tasks::holder::{self, Holder};
use tasks::tasks::reference::TaskRef;
use tasks::tasks::selection::list_query;
use tasks::tasks::types::Priority;

/// Where the service lives. The VPN name, because that is the only place it is.
const DEFAULT_URL: &str = "https://tasks.xinutec.org";

/// The two model facts a reader cannot guess, and what the five ranks mean.
///
/// ⚠ **Assembled rather than written out**, because [`Priority::gloss`] is where
/// the levels are defined and a second copy in a help string is the copy that
/// drifts. Five names two readers interpret differently rank nothing.
fn long_about() -> String {
    let levels: String = Priority::all()
        .iter()
        .map(|p| format!("\n  {p}  {}", p.gloss()))
        .collect();
    format!(
        "The work Claude sessions and Pippijn hand between each other.

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

UNASSESSED is not a sixth level — it sorts exactly where P2 does. So P0 and P1
rise above it and P3 and P4 sink below, and anything nobody has judged keeps its
place: oldest first, which is what makes old work get fixed rather than buried.

The escape is there so the required flag cannot be satisfied by typing P2 at a
question you did not answer. Everything on P0 and everything on P2 fail the same
way — the rank stops carrying information."
    )
}

/// What `task edit --help` says under the flags.
///
/// ⚠ **Assembled, because [`density::RUBRIC`] is graded against.** The same
/// three rules are put to the model that reads a body which has grown without
/// being consolidated, and a second copy here is the copy that drifts — leaving
/// sessions written to one standard and marked against another. A standard
/// stated only to the judge arrives after the writing; here it arrives before,
/// which is the only place it can prevent anything.
fn edit_about() -> String {
    format!(
        "Change a task's words.

⚠ **Rewriting is the moment to put the conclusion back on top.** A body grows in
the order things happened, so what is still true sinks to the bottom: measured
across every task with one, #704's verdict sat 98% down its 132 lines and #697's
plan for a new machine was the last paragraph of a ticket about copying a disk.
Lead with where it stands; the history goes under it. Better still, put the
state in `--subject`, which is the only part anybody reads without opening the
task.

⚠ **`--prepend` is how to do that in one command, and `--body` is not.** `--body`
replaces everything; recording an outcome with it deletes the filing unless the
old text is read out and pasted back first. On 2026-08-15 that read was skipped
twice in one afternoon by the session that maintains this tool — once caught by
the guard on `--body`, once under its threshold at 52% kept, where nothing
catches it.

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
    about = "The work Claude sessions and Pippijn hand between each other",
    long_about = long_about()
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
    /// A task is `{id, subject, status, assignee, detailed, filed_by,
    /// created_at, updated_at, closed_at}`, plus `priority` when one is set —
    /// `P0` to `P4`, and ABSENT rather than null on the almost-everything that
    /// has none. `status` is one of `open`, `doing`, `done`, `dropped`. THE
    /// HOLDER IS `assignee`, an object — `{kind, id,
    /// name}` with `kind` one of `session`, `person`, `nobody` — and there is no
    /// top-level `session` field.
    ///
    /// ⚠ **That last sentence is here because guessing it cost a wrong answer.**
    /// A session filtering `--all --json` by hand assumed `session` and reported
    /// 137 tasks in the pile against a real 5, since every row lacks a field
    /// that does not exist. `--pile` now answers that question directly; this
    /// says what the shape is for the questions that have no flag.
    ///
    /// ⚠ **THAT IS THE LIST SHAPE, AND `show --json` IS A BIGGER ONE.** A row in
    /// a list carries no prose — `detailed` is the BOOLEAN standing in for it,
    /// answered in SQL so forty bodies do not cross the wire to report forty
    /// yes/nos. `show --json` adds the real thing: `body`, `body_html`, `events`,
    /// `restorable`. Reading `detailed` where you meant `body` yields `true`, and
    /// piping that into `edit --body` writes the string `True` over the prose —
    /// which happened on 2026-08-15 and cost a `task undo`. **`show <id> --body`
    /// prints the body alone**, which is what a script patching one wants, and
    /// `--previous --body` gives the version before the last edit to diff against.
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
        /// Strictly what nobody holds: the pile, and what is going spare.
        ///
        /// Your prompt shows at most five of these, so this is where the rest
        /// are. Needs no session id — the pile belongs to no conversation.
        #[arg(long, conflicts_with_all = ["all", "mine"])]
        pile: bool,
        /// Include finished tasks.
        #[arg(long)]
        done: bool,
    },
    /// Work on a few things and let the rest go quiet for a while.
    ///
    ///     task focus 849 850 --for 4h
    ///
    /// For those four hours **your prompt** recites only those tasks and counts
    /// the rest. A session with fifty open tasks pays for all fifty on every
    /// turn and is working on two of them; this is how to say which two.
    ///
    /// ⚠ **`task list` is unaffected and still shows everything.** The digest is
    /// the channel nobody asked for; a list you typed is one you wanted, and the
    /// question "what should I pick up next" must never be answered with
    /// silence.
    ///
    /// ⚠ **P0 and overdue break through**, so a focus cannot bury the one thing
    /// that was meant to interrupt it.
    ///
    /// ⚠ **It expires and there is no way to say "until I say otherwise".** A
    /// focus you forget stops applying at its hour. Longer than a day is not a
    /// focus but a handover — `task move` is how work changes hands where
    /// everybody can see it.
    ///
    /// Naming no task asks what the focus is; `--clear` ends it now.
    Focus {
        /// What you are working on. Repeat for several; the whole set is
        /// replaced, so this states what you are on and never adds to it.
        ids: Vec<TaskRef>,
        /// How long: `4h`, `90m`, `2h30m`. A bare number is minutes.
        ///
        /// Required when naming tasks — there is no default, because the
        /// expiry is the only thing that makes hiding an open task safe.
        #[arg(long = "for")]
        period: Option<String>,
        /// End it now, before its hour.
        #[arg(long, conflicts_with_all = ["ids", "period"])]
        clear: bool,
    },
    /// One task, with its prose and its history.
    Show {
        id: TaskRef,
        /// Print the body alone, with no header and no history — for diffing
        /// prose against another copy of it, which is what checking a migration
        /// consists of.
        #[arg(long, conflicts_with = "json")]
        body: bool,
        /// Show the task as it stood before its most recent edit.
        ///
        /// Composes with `--body`, which is the shape a diff wants:
        /// `task show 25 --previous --body > was.md`.
        #[arg(long)]
        previous: bool,
    },
    /// Put back the subject and body the most recent edit replaced.
    ///
    /// ⚠ **Undo is itself an edit**, recorded in the history like any other and
    /// leaving its own previous version behind — so undoing an undo works, and
    /// nothing here is a special path that steps around the record.
    ///
    /// It restores **both** the subject and the body, because that is what a
    /// revision is: the task as it stood, not a column. Look first with
    /// `task show <id> --previous` if that is not what you want.
    ///
    /// ⚠ **It reverts THE last edit, not YOUR last edit** — one version is kept
    /// per task, not per actor. Where the last edit was somebody else's this
    /// refuses; `--anyway` is how to mean it.
    Undo {
        id: TaskRef,
        /// Revert the last edit even though another conversation made it.
        #[arg(long)]
        anyway: bool,
    },
    /// File a task.
    ///
    /// ⚠ **A filing must state urgency**, either `--priority` or
    /// `--unassessed`. Both are answers; leaving both off is not, and is
    /// refused before anything reaches the service. Pippijn, 2026-08-11: "I
    /// want everything to have a priority."
    #[command(group(clap::ArgGroup::new("rank").required(true).args(["priority", "unassessed"])))]
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
        /// How urgent: P0 to P4. Required, unless you say `--unassessed`.
        ///
        /// The levels are under `task --help`. Read them before picking: they
        /// are tests a task either passes or does not, and `P2` is the one to
        /// reach for when none of the others fit.
        #[arg(long)]
        priority: Option<Priority>,
        /// Say, explicitly, that you are not judging this one.
        ///
        /// ⚠ **Not a sixth level and not a shrug.** It sorts exactly where `P2`
        /// does; the difference is that `P2` claims somebody read the task and
        /// called it ordinary, and this claims nobody has. Use it when the work
        /// is not yours to judge — filing into another session's domain is the
        /// ordinary case — and leave the call to whoever picks it up.
        ///
        /// It exists so that the required flag above cannot be satisfied by
        /// typing `P2` at a question you did not answer.
        #[arg(long, conflicts_with = "priority")]
        unassessed: bool,
        /// Task ids this one waits for. Repeat for several.
        #[arg(long = "blocked-on")]
        blocked_on: Vec<u64>,
        /// The day it has to be done by: YYYY-MM-DD.
        #[arg(long)]
        due: Option<NaiveDate>,
        /// File it even though something open already says this.
        ///
        /// ⚠ **Both halves refuse, so this is the only way past either.** An
        /// open task with the same subject is caught by string equality; the
        /// ones only a reader would spot are caught by a Haiku call, which costs
        /// 8-25 seconds before the task is filed. Neither is a guess you cannot
        /// overrule — that is what this is for, and the body you were filing is
        /// still in the command you just ran, so overruling is one re-run.
        ///
        /// Also the flag for filing a batch, and for a machine with no `claude`
        /// on its PATH — though that case needs no flag, since a check that
        /// cannot run files the task and says so.
        #[arg(long)]
        no_duplicate_check: bool,
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
    /// Put a closed or started task back to open.
    ///
    /// The fourth status had three verbs. `done` and `drop` close a task and
    /// `start` moves it along; nothing came back, so un-closing one meant a
    /// hand-rolled PATCH — or dropping and refiling it, which throws the history
    /// away. #700.
    ///
    /// ⚠ **It leaves the holder alone**, which is the service's rule and not
    /// this command's: whoever last had it is a better guess than nobody. So
    /// reopening something you finished puts it back on your own plate rather
    /// than in the pile — `task move <id> nobody` is how it goes back for
    /// whoever picks it up.
    Reopen { id: TaskRef },
    /// Hand a task over: `me` (this conversation), `pippijn`, `nobody`, or a
    /// session — **by name or by id**, whichever you have.
    ///
    /// `task move 42 observe` works, because every list prints the name. Until
    /// 2026-08-10 only the id was accepted, so this tool's own output was not
    /// valid input to it and a handover meant grepping `task sessions` first.
    /// A name that matches nothing, or matches two conversations, is refused
    /// rather than guessed — names are reused, and the service's own answer to
    /// an id it does not know is a 500 reading `moving a task`, which says
    /// nothing about what was wrong with it.
    ///
    /// ⚠ **Handing work to a quiet conversation is queueing, not stranding.** A
    /// session never ends — it goes offline and comes back — so there is no such
    /// thing as giving a task to one that has "gone", and its open list is the
    /// work waiting for it rather than work it is doing. Pick the holder by
    /// whose subject it is; whether anybody is at that keyboard now is not a
    /// property of the task and does not belong in the decision.
    ///
    /// `nobody` is for work that genuinely suits whichever conversation is
    /// around next. It is not the safe default for "I am not sure they are
    /// still there" — the pile is a handover channel, not a lost-property
    /// office, and it costs every session's prompt rather than one.
    Move { id: TaskRef, to: To },
    /// Change a task's words.
    #[command(long_about = edit_about())]
    Edit {
        id: TaskRef,
        #[arg(long)]
        subject: Option<String>,
        /// `-` reads stdin. REPLACES the whole body — `--prepend` is how to
        /// keep it.
        #[arg(long, conflicts_with_all = ["prepend", "append"])]
        body: Option<String>,
        /// Put text ABOVE the body, keeping every word of it. `-` reads stdin.
        ///
        ///     task edit 42 --prepend "DONE in a2c3ab6 — deployed and verified."
        ///
        /// ⚠ **This is almost always the one you want**, and not only because
        /// it is safe: a body grows in the order things happened, so what is
        /// still true sinks to the bottom. Lead with where it stands and let
        /// the history sit under it.
        #[arg(long)]
        prepend: Option<String>,
        /// Put text BELOW the body, keeping every word of it. `-` reads stdin.
        ///
        /// For when what you are adding really is the next thing that happened
        /// rather than the conclusion. Composes with `--prepend`.
        #[arg(long)]
        append: Option<String>,
        /// Skip the read a model gives a body that has grown without being
        /// consolidated.
        ///
        /// It never refuses a write, so this is for a script that wants neither
        /// the wait nor the words — not for getting an edit past it.
        #[arg(long = "no-density-check")]
        no_density_check: bool,
        /// Mean it, where `--body` would leave almost nothing of a substantial
        /// one.
        ///
        /// That write is refused by default, because it is far more often a
        /// mistake than an edit — on 2026-08-15 a session took `detailed` from
        /// `--json` for the prose and wrote `True` over 3,109 characters of
        /// #900. Emptying a body on purpose is what this flag is for.
        #[arg(long = "replace-body")]
        replace_body: bool,
        /// Rank it: P0 to P4, listed under `task --help`.
        ///
        /// There is no way to UNRANK from here, deliberately: absence means
        /// "leave it alone" for every other field on this command, and a task
        /// ranked wrongly is corrected by ranking it again.
        #[arg(long)]
        priority: Option<Priority>,
        /// What this task waits for. Repeat for several; the whole set is
        /// replaced, so `--unblock` is the way to say "nothing".
        ///
        /// A task may not be ranked more urgently than what blocks it, and
        /// nothing may block itself or close a loop. Both are refused with the
        /// other task named.
        #[arg(long = "blocked-on", conflicts_with = "unblock")]
        blocked_on: Vec<u64>,
        /// It is not waiting for anything any more.
        #[arg(long)]
        unblock: bool,
        /// The day it has to be done by: YYYY-MM-DD.
        ///
        /// A deadline does NOT reorder anything — the list is still ordered by
        /// priority. It is evidence for a rank, not a substitute for one. What
        /// it does refuse is a date earlier than something this task is blocked
        /// on, which cannot be met whatever anybody decides.
        #[arg(long, conflicts_with = "no_due")]
        due: Option<NaiveDate>,
        /// Take the deadline off.
        #[arg(long = "no-due")]
        no_due: bool,
    },
    /// Exactly what a prompt receives — for checking the cost, not for reading.
    Digest,
    /// Who holds what: each session that has, Pippijn, and the pile — open/total.
    Sessions {
        /// Every conversation there has ever been, including those that have
        /// never been given anything — which is most of them, since a row is
        /// created by a session's first prompt. This is where a brand-new
        /// conversation's id can be found to hand it work.
        #[arg(long)]
        all: bool,
    },
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

/// What Claude Code is calling this conversation, right now.
///
/// ⚠ **This is why a session no longer has to name itself.** The name is on
/// this disk already — the CLI writes it into the transcript and appends another
/// whenever it changes — so asking a conversation to type `task rename` was
/// asking it to do a computer's job, and the one holding the most open work had
/// never got round to it. See [`tasks::agent_name`] for the shapes and the
/// measurements.
///
/// Silent on every failure. No `HOME`, no transcript, a CLI that has changed the
/// line: the service keeps whatever name it already had, which is exactly the
/// behaviour that existed before this.
fn called_now(session: &str) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let projects = std::path::Path::new(&home).join(".claude").join("projects");
    tasks::agent_name::from_projects(&projects, session)
}

struct Client {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
    session: Option<String>,
    /// What Claude Code calls this conversation, when its transcript says.
    ///
    /// Resolved once per command rather than per request: it is a bounded read
    /// of a local file, and a second command is a second process anyway.
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
        // Sent on every request, including reads: a session that only ever
        // looks at its list still gets named, and a rename in Claude Code
        // reaches the service on the next command without anybody typing it.
        if let Some(called) = &self.called {
            req = req.header(tasks::access::SESSION_NAME_HEADER, called);
        }
        req
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Option<Value>> {
        // Built rather than sent, so the method can be read: anything that is
        // not a GET has changed the list, and the prompt hook is holding an
        // answer from before it. Central here rather than in each command,
        // because the one that forgets is the one that files a duplicate.
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
        if wrote {
            self.forget_cached_digest();
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

    /// Turn what somebody typed after `move` into a session id.
    ///
    /// ⚠ **Every place this tool PRINTS a holder, it prints the name** — `(coach)`,
    /// `(observe)` — and until 2026-08-10 the only thing it ACCEPTED was a
    /// 36-character id. Its own output was not valid input to it, so handing over
    /// five tasks meant first running `task sessions | grep` to translate three
    /// names into uuids, and pasting them.
    ///
    /// ⚠ **It refuses rather than falling through to "probably an id".** The
    /// write itself would not land — a foreign key stands behind
    /// `assignee_session` — but it fails as a 500 whose whole message is
    /// `moving a task`, which sends the reader to look at the service. Refusing
    /// here answers the actual question, with the names to hand.
    async fn resolve(&self, to: To) -> Result<To> {
        let To::Session(typed) = &to else {
            return Ok(to);
        };
        // Holders first: it is the short list, and handing work to a
        // conversation that already carries some is the ordinary case.
        let mut known = known_sessions(
            &self
                .send(self.request(reqwest::Method::GET, "/api/holders"))
                .await?
                .unwrap_or(json!([])),
        );
        if !known.iter().any(|(id, name)| matches(id, name, typed)) {
            // Every row there is — 717 against 14 when that was split — asked
            // for only when the cheap list did not answer.
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
            Holder::Session(id) => Ok(To::Session(id)),
            Holder::Unknown(names) => bail!(
                "no session called `{typed}`, and it is not an id this service knows. \
                 Assigning it anyway would hand the task to a conversation that is not \
                 there, which leaves every list but `--all`. Known: {}",
                names.join(", ")
            ),
            Holder::Ambiguous(ids) => bail!(
                "`{typed}` is the name of {} conversations, so this would guess: {}. \
                 Give the id instead — `task sessions` prints both.",
                ids.len(),
                ids.join(", ")
            ),
        }
    }

    /// Drop the prompt hook's copy, so the next prompt shows what just changed.
    ///
    /// Silent on every failure, including no `HOME`: this runs after a write
    /// that has already succeeded, and the cost of missing it is that one
    /// prompt is up to a minute behind.
    fn forget_cached_digest(&self) {
        let (Some(session), Ok(home)) = (&self.session, std::env::var("HOME")) else {
            return;
        };
        tasks::hook::forget_digest(std::path::Path::new(&home), session);
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
    /// A conversation, by its id **or by its name**.
    ///
    /// Which of the two is not decided here: telling them apart needs the
    /// service, and this is a `FromStr`. [`Client::resolve`] settles it before
    /// anything is sent.
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

/// Whether a row answers to what somebody typed, as an id or as a name.
fn matches(id: &str, name: &Option<String>, typed: &str) -> bool {
    id == typed || name.as_deref() == Some(typed)
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

/// The model that reads the list. The cheapest one there is, deliberately:
/// this rides the same subscription allowance as the session that ran the
/// command, and a second opinion about a title is not worth taking room from
/// the work. Named in full rather than by alias so that changing model is a
/// change to this line.
const CHECKER: &str = "claude-haiku-4-5-20251001";

/// How long one check may take before it is abandoned.
///
/// ⚠ **120 seconds, because 60 was inside the spread rather than outside it.**
/// The first bound came from five replayed filings at 8–24 seconds against 134
/// open tasks. On 2026-08-23 the same call took 54 and 73 seconds on a 3.8 kB
/// body while #982's 100 kB came back in 20, and five filings in the transcripts
/// since 2026-08-14 died on the bound — against 280 filed. What varies is the
/// one-shot session's startup, not the reading, so a bound near the median
/// abandons calls that would have answered. Only that tail pays the wider one.
///
/// Provisional, and it is the last number here that will be a guess:
/// [`checks`](tasks::tasks::checks) now records the elapsed time of every call,
/// so the next reading comes from a distribution.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(120);

/// The same, for reading a body rather than a list of titles.
///
/// ⚠ **Longer, and NOT because a bigger body takes longer.** Two measurements,
/// 2026-08-23: 54 seconds on a 3.8 kB body and 20 seconds on the 100 kB of
/// #982. What varies is the one-shot session's own startup, not the reading, so
/// the small-body case came within six seconds of [`PATIENCE`] and would have
/// been abandoned by bad luck alone. The filing check cannot afford this, since
/// it runs before the write and its timeout is latency a session pays to file;
/// this one runs after, where the only thing waiting is a terminal.
const READING: std::time::Duration = std::time::Duration::from_secs(150);

/// Every open task, as the id and title a duplicate would be spotted by.
///
/// ⚠ **Every open task, not this session's.** The duplicate that matters is the
/// one another conversation filed — it is the only kind nobody can see — so this
/// deliberately asks the widest question the list supports, and is the one place
/// in this CLI that does. `--all`'s reasoning in `selection.rs` is about what
/// costs a prompt its bytes; this costs one command's stderr.
///
/// ⚠ **Read before the POST, not after.** It used to run afterwards and filter
/// the new id back out of its own answer. Reading it first is what lets
/// [`duplicates::same_subject`] refuse a collision while there is still nothing
/// to undo; the slow half is unaffected, because it runs against this same list
/// once the filing has landed.
async fn open_now(client: &Client) -> Result<Vec<(u64, String)>> {
    let query = list_query(true, false, false, false, None)?;
    let req = client
        .request(reqwest::Method::GET, "/api/tasks")
        .query(&query);
    let open = client.send(req).await?.unwrap_or(json!([]));
    Ok(open
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|task| Some((task["id"].as_u64()?, task["subject"].as_str()?.to_string())))
        .collect())
}

/// Whether anything open already says what is about to be filed.
///
/// `corpus` is read before the filing and so cannot contain it, which is what
/// lets this refuse: there is nothing to undo yet.
async fn already_filed(
    client: &Client,
    corpus: &[(u64, String)],
    subject: &str,
) -> Result<Vec<duplicates::Match>> {
    if corpus.is_empty() {
        return Ok(Vec::new());
    }
    let asked = duplicates::prompt(subject, corpus);
    let input_chars = asked.chars().count().min(u32::MAX as usize) as u32;
    let (said, elapsed_ms) = ask(&asked, PATIENCE).await;
    let found = match &said {
        Ok(words) => duplicates::parse(words, corpus),
        Err(_) => Vec::new(),
    };
    recorded(
        client,
        checks::Run {
            kind: checks::Kind::Filing,
            task_id: None,
            input_chars,
            accreted: None,
            elapsed_ms,
            outcome: checks::outcome(&said, !found.is_empty()),
        },
    )
    .await;
    // The failure is still the caller's to print: it is the line that says a
    // filing went unchecked, and a row in a table nobody is reading tonight does
    // not tell the session in front of it.
    said?;
    Ok(found)
}

/// Report one run, and never let reporting it cost anything.
///
/// Silent on every failure, for the same reason the checks themselves are: this
/// runs after the call it describes, so there is nothing left to protect and a
/// session that cannot reach the service has a worse problem than a missing row.
async fn recorded(client: &Client, run: checks::Run) {
    let req = client
        .request(reqwest::Method::POST, "/api/checks")
        .json(&run);
    let _ = client.send(req).await;
}

/// Put the question to a one-shot session and leave nothing behind.
///
/// ⚠ **Every call is a conversation, and a conversation is a file that outlives
/// it.** memview's `console/src/gist.rs` found 2,299 of these and 57 MB in the
/// three days after its own sweep was written, because a `claude -p` call files
/// a transcript like any other session and nothing was removing them. So the id
/// is named here rather than left to the CLI, and the file goes the moment the
/// answer is in hand — including on the failing paths, which leave exactly the
/// same file as the working one.
async fn ask(prompt: &str, patience: std::time::Duration) -> (Result<String>, u32) {
    let named = named();
    let started = std::time::Instant::now();
    let said = call(prompt, &named, patience).await;
    // Before `discard`, which is a file removal on the same path and no part of
    // what was being measured.
    let took = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    discard(&named);
    (said, took)
}

/// The call itself, up to the words that came back.
///
/// ⚠ **On stdin, not in the argument list.** The prompt carries every open
/// title — 13,720 bytes when this was written — and an argument that size is at
/// the mercy of a shell's limits and of anything that logs a command line.
async fn call(prompt: &str, named: &str, patience: std::time::Duration) -> Result<String> {
    let mut child = tokio::process::Command::new("claude")
        .current_dir(std::env::temp_dir())
        .arg("-p")
        .args(["--session-id", named])
        .args(["--model", CHECKER])
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
    // Closed, because `-p` reads until end of file and would otherwise wait for
    // the rest of a prompt that has already been sent in full.
    drop(stdin);
    let out = tokio::time::timeout(patience, child.wait_with_output())
        .await
        .with_context(|| format!("no answer in {}s", patience.as_secs()))?
        .context("waiting for claude")?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A session id for one of these calls.
///
/// A version-4 UUID by hand rather than a dependency: `rand` is already in the
/// tree and this is the only place in the project that needs one.
fn named() -> String {
    let mut bytes = [0u8; 16];
    rand::fill(&mut bytes);
    // Version 4, variant 1 — the CLI validates the shape of what it is given.
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

/// Remove the transcript one of these calls left behind.
///
/// Silent when there is nothing to remove. A CLI that never got as far as
/// writing a file, or one that files them somewhere else entirely, is not a
/// failure of the filing this was checking.
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
    // Before the subject rather than after it: a column of ranks is scannable
    // down the left edge, and the list is already sorted so they arrive in
    // order. Two spaces where there is no rank, so nothing shifts sideways
    // between a ranked line and an unranked one.
    // A near deadline raises the rank; `!` marks a level nobody chose, so the
    // order never reads as random. The rule lives in SQL — this only prints
    // whichever value the service says the list was sorted by.
    let rank = match task["escalated_to"].as_str() {
        Some(raised) => format!("{raised}!"),
        None => task["priority"].as_str().unwrap_or("").to_string(),
    };
    let mut out = format!(
        "{marker} #{:<4} {rank:<3} {}",
        task["id"].as_u64().unwrap_or(0),
        task["subject"].as_str().unwrap_or("")
    );
    if let Some(due) = task["due"].as_str() {
        if task["overdue"].as_bool().unwrap_or(false) {
            out.push_str(&format!("  OVERDUE {due}"));
        } else {
            out.push_str(&format!("  due {due}"));
        }
    }
    if task["blocked"].as_bool().unwrap_or(false)
        && let Some(on) = task["blocked_on"].as_array()
    {
        let ids: Vec<String> = on
            .iter()
            .filter_map(|v| v.as_u64())
            .map(|v| format!("#{v}"))
            .collect();
        out.push_str(&format!("  ⛔{}", ids.join(",")));
    }
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
    let session = cli.session.clone().or_else(session_id);
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
        called: session.as_deref().and_then(called_now),
        session,
    };
    client.identified()?;

    match cli.command {
        Command::List {
            all,
            mine,
            pile,
            done,
        } => {
            let query = list_query(all, mine, pile, done, client.session.as_deref())?;
            let req = client
                .request(reqwest::Method::GET, "/api/tasks")
                .query(&query);
            let tasks = client.send(req).await?.unwrap_or(json!([]));
            emit(cli.json, &tasks, || {
                let tasks = tasks.as_array().cloned().unwrap_or_default();
                if tasks.is_empty() {
                    // Which question came back empty, because the three read
                    // very differently: an empty pile is the fleet keeping up,
                    // and an empty plate is this session having nothing to do.
                    println!(
                        "{}",
                        if pile {
                            "the pile is empty"
                        } else {
                            "nothing open"
                        }
                    );
                }
                for task in &tasks {
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
                // The header says WHEN this stopped being the task, not when it
                // was written: a previous version's only useful timestamp is the
                // edit that displaced it, which is what `task undo` reverses.
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
            // Read before the write, so a refusal costs nothing. `mine` is the
            // service's answer about stored identity — the label beside it is
            // for reading, not for comparing.
            if !anyway {
                let was: tasks::tasks::types::Revision = serde_json::from_value(was.clone())
                    .context("the service's previous version did not parse")?;
                if tasks::tasks::undo::needs_saying(&was) {
                    bail!(tasks::tasks::undo::refusal(&was, id.id()));
                }
            }
            // `replace_body` because putting a body back is an informed
            // replacement, and the guard on collapsing bodies would otherwise
            // refuse the undo of an edit that had made one longer.
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
            priority,
            // Read by clap's group, not here: `--unassessed` is the absence of
            // `--priority` once one of the two is known to have been given.
            unassessed: _,
            blocked_on,
            due,
            no_duplicate_check,
        } => {
            client.writing()?;
            // The list comes first, and two separate questions are asked of it:
            // whether something open carries this exact subject, and whether a
            // model sees the same problem in different words. Both are answered
            // before the POST and both refuse.
            //
            // ⚠ **A list that could not be read costs a note, never a filing.**
            // Moving this ahead of the POST would otherwise have turned every
            // transport failure into a refused filing.
            let corpus = match no_duplicate_check {
                true => Vec::new(),
                false => open_now(&client).await.unwrap_or_else(|why| {
                    eprintln!("(duplicate check did not run: {why:#})");
                    Vec::new()
                }),
            };
            if let Some(already) = duplicates::same_subject(&subject, &corpus) {
                bail!("{}", duplicates::collision(already));
            }
            // ⚠ **The model runs BEFORE the POST, because a refusal it comes
            // after is not a refusal.** This cost the filing 8-25 seconds of
            // latency it used to spend after the task already existed; that is
            // the price of the default Pippijn asked for on 2026-08-14.
            //
            // ⚠ **A check that could not run files the task.** Only a model that
            // actually named something refuses. A missing `claude`, a timeout or
            // an unreadable answer must never cost a filing — a session that
            // cannot write things down is worse than any duplicate.
            if !no_duplicate_check {
                match already_filed(&client, &corpus, &subject).await {
                    Ok(found) if !found.is_empty() => bail!("{}", duplicates::refusal(&found)),
                    Ok(_) => {}
                    Err(why) => eprintln!("(duplicate check did not run: {why:#})"),
                }
            }
            let mut payload = json!({ "subject": subject, "body": raw.as_deref().map(body).transpose()?.unwrap_or_default() });
            // Always present, and null when unassessed: the service refuses a
            // filing that never mentions it, so there is no "leave it out" arm
            // here to fall down. clap's group guarantees exactly one of the two
            // arrived, which is why `unassessed` needs no test of its own.
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
            let req = client
                .request(reqwest::Method::POST, "/api/tasks")
                .json(&payload);
            let task = client.send(req).await?.context("no task came back")?;
            emit(cli.json, &task, || println!("{}", line(&task)));
            // After the filing and never before it. The task is on the list by
            // the time a model is asked anything, so a check that hangs, fails
            // or is not installed costs a note and never a filing.
        }

        Command::Start { id } => {
            patch(&client, cli.json, id, json!({ "status": "doing" })).await?;
        }
        Command::Done { id, to } => {
            let mut change = json!({ "status": "done" });
            if let Some(to) = to {
                let to = client.resolve(to).await?;
                change["assignee"] = assignee(&to, client.me()?);
            }
            patch(&client, cli.json, id, change).await?;
        }
        Command::Drop { id } => {
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
            // An empty list IS the clear, so `--unblock` and `--blocked-on`
            // reach the service as the same field with different contents.
            if unblock {
                change["blocked_on"] = json!([] as [u64; 0]);
            } else if !blocked_on.is_empty() {
                change["blocked_on"] = json!(blocked_on);
            }
            // A date has no "empty" value the way a blocker list does, so
            // removing one needs its own word on the wire.
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
                // Only alongside a body, so the flag cannot be left on a shell
                // line that no longer writes one.
                if replace_body {
                    change["replace_body"] = json!(true);
                }
            }
            // ⚠ **There is one stdin, so only one of these may claim it.** The
            // second `body("-")` would read an exhausted stream and add an
            // empty string, which the service then refuses with a message about
            // an empty variable — true, and no help at all in finding this.
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
            // After the write and never before it, like the duplicate check
            // after a filing: this is advice about prose, and a check that
            // hangs, fails or is not installed must cost a note at most.
            if !no_density_check && !cli.json {
                accreting(&client, id, &updated).await;
            }
        }

        Command::Focus { ids, period, clear } => {
            if clear {
                let req = client.request(reqwest::Method::DELETE, "/api/focus");
                let was = client.send(req).await?.unwrap_or(json!({}));
                emit(cli.json, &was, || {
                    match was["was"].is_null() {
                        false => println!("focus ended — your prompt shows everything open again"),
                        // Not an error: the caller asked to be unfocused and is.
                        true => println!("there was no focus on"),
                    }
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
                let period = tasks::tasks::focus::parse(&period)?;
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
                    // Nothing to fall back to: a POST that answered 2xx with a
                    // shape this cannot read is a disagreement about the API,
                    // and saying "focused" anyway would report a state nobody
                    // has confirmed.
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

        Command::Sessions { all } => {
            // Two questions, two routes. `/api/holders` is who is carrying what
            // and leaves out the conversations that have never carried
            // anything; `/api/sessions` is every row there is, which is every
            // conversation that has ever run — 717 against 14 when this was
            // split. The second answers "what is this new session's id", and
            // nothing else, which is why it is asked for rather than given.
            let path = if all { "/api/sessions" } else { "/api/holders" };
            let req = client.request(reqwest::Method::GET, path);
            let rows = client.send(req).await?.unwrap_or(json!([]));
            emit(cli.json, &rows, || {
                for holder in rows.as_array().cloned().unwrap_or_default() {
                    // `open/total`, not `open`: a bare 0 reads as an idle session,
                    // and `0/56` is one that has cleared its plate. The id is the
                    // handle for `task move`, so it stays in the line even though
                    // the name is what is read.
                    //
                    // A session row has no history to report, so `--all` prints
                    // the open count alone rather than `3/0`, which would say
                    // the session had never finished anything.
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
            // ⚠ Refused rather than accepted-and-reverted. Every request this
            // CLI makes carries the name Claude Code is using, and the service
            // takes the newer one — so a rename to something else would print
            // its success line and be gone by the next command. That is the
            // exact shape this repository has now fixed three times, and the
            // remedy is to say where the lever actually is.
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

/// Change a task, named either way.
///
/// ⚠ **A write that moved nothing says so**, because until 2026-08-10 it printed
/// a line identical to the one a real change produces. `task start` on a task
/// already `doing` in the pile claimed nobody and looked like it had worked;
/// so did a rename to a blank name, and closing into the pile. Each was found
/// by reproducing it on a scratch task rather than by the caller noticing.
///
/// It is a note, not an error: a no-op is often the right answer — starting a
/// task already yours is meant to be quiet — and a non-zero exit would turn a
/// silent success into a spurious failure. The service reports which
/// `task_events` it wrote; empty means none.
/// The task as it stood before its last edit, or why there is no such thing.
///
/// ⚠ **The service answers 404 for two different states** — a task that does
/// not exist, and one nothing has ever overwritten — and from here they are the
/// same answer: there is nothing to put back. The message says both, because a
/// reader who mistypes an id and a reader whose task predates the revision
/// table would otherwise draw opposite conclusions from one line.
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

/// The focus a route answered with, when it answered with one.
///
/// `null` is the ordinary answer — almost no session is focused at any moment —
/// so this is an absence rather than a failure, and the caller says what "not
/// focused" reads like in its own context.
fn parse_focus(value: &Value) -> Option<tasks::tasks::focus::Focus> {
    serde_json::from_value(value.clone()).ok()
}

/// A focus as one line: what, until when, and how much is left of it.
///
/// ⚠ **The countdown is here and never in the digest.** This runs the moment
/// somebody types the command, where "1h48m left" is true; the digest is cached
/// for sixty seconds and read minutes later, so it prints the hour instead.
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

async fn patch(client: &Client, json: bool, id: TaskRef, change: Value) -> Result<Value> {
    client.writing()?;
    let id = id.id();
    let req = client
        .request(reqwest::Method::PATCH, &format!("/api/tasks/{id}"))
        .json(&change);
    let task = client.send(req).await?.context("no task came back")?;
    emit(json, &task, || {
        println!("{}", line(&task));
        if task["changed"].as_array().is_some_and(|c| c.is_empty()) {
            println!("nothing changed — it was already like that");
        }
        if let Some(was) = displaced(&task, id) {
            println!("{was}");
        }
    });
    Ok(task)
}

/// A model's reading of a body that has grown without anybody rewriting it.
///
/// ⚠ **Every failure here is silence.** The edit has already landed, so there is
/// nothing left for this to protect: a missing `claude`, a timeout, a body that
/// could not be re-read are all reasons to say nothing rather than to spend a
/// session's attention on the checker. `duplicates.rs` may cost a filing an
/// error line because it runs BEFORE the write and can still stop one; this
/// cannot stop anything, so it is advice or it is quiet.
async fn accreting(client: &Client, id: TaskRef, updated: &Value) {
    let Some(accreted) = updated["replaced"]["accreted"].as_u64() else {
        return;
    };
    if !density::worth_asking(accreted as usize) {
        return;
    }
    // Re-read rather than reassembled from what was sent: `--prepend` and
    // `--append` each carry a fragment, and the thing being judged is the whole
    // document as it now stands.
    let req = client.request(reqwest::Method::GET, &id.path());
    let Ok(Some(task)) = client.send(req).await else {
        return;
    };
    let Some(body) = task["body"].as_str() else {
        return;
    };
    let asked = density::prompt(id.id(), accreted as usize, body);
    let input_chars = asked.chars().count().min(u32::MAX as usize) as u32;
    let (said, elapsed_ms) = ask(&asked, READING).await;
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
        },
    )
    .await;
    if let Some(advice) = advice {
        eprintln!("{advice}");
    }
}

/// What an edit landed on, said back to whoever made it.
///
/// ⚠ **This is the line that would have stopped the loss, and it stops
/// nothing.** On 2026-08-14 a session rewrote a body from a snapshot it had
/// read three days earlier; it believed the body was from 08-11, and the task
/// had been rewritten twice since. Told at the moment of the write that it had
/// just replaced text written *yesterday, by somebody else*, the mismatch is
/// there to see. Refusing instead is the wrong trade for the same reason
/// `duplicates.rs` gives: rewriting another session's words is a permitted
/// operation performed often, and a gate on a frequent correct operation
/// teaches everyone to pass it.
///
/// The undo comes with it, because knowing a mistake was made is only half of
/// it — this is the moment the remedy is wanted, and it is one command.
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
