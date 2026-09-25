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
//! request that does not say which conversation it is (`access.rs`); this CLI
//! stops before the round trip and says which half — token, identity — is
//! missing.
//!
//! **Naming a task.** Every command that takes one accepts a bare number, or the
//! `#`-prefixed form the digest prints.
//!
//! ```text
//! task list [--all|--mine|--pile] [--done] yours and the pile; wider; narrower; spare
//! task list --handed-out          what you filed and somebody else is holding
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
use tasks::tasks::commands;
use tasks::tasks::density;
use tasks::tasks::duplicates;
use tasks::tasks::fleetwatch;
use tasks::tasks::focus;
use tasks::tasks::holder::{self, Holder};
use tasks::tasks::reference::TaskRef;
use tasks::tasks::selection::{self, list_query};
use tasks::tasks::types::{AssigneeKind, Priority, Status, Task};
use tasks::tasks::wait;

/// Where the service lives. The VPN name, because that is the only place it is.
const DEFAULT_URL: &str = "https://tasks.xinutec.org";

/// The two model facts a reader cannot guess, and what the five ranks mean.
///
/// ⚠ **Assembled rather than written out**: [`Priority::gloss`] defines the
/// levels, and a second copy here would drift.
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

/// What `task edit --help` says under the flags.
///
/// ⚠ **Assembled from [`density::RUBRIC`], never restated**, or sessions would
/// write to one standard and be marked against another.
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
    /// A task carries `id`, `subject`, `status` (`open`, `doing`, `done`,
    /// `dropped`) and `assignee`; optional fields such as `priority` (`P0` to
    /// `P4`) are ABSENT, not null, when unset. THE HOLDER IS `assignee`, an
    /// object — `{kind, id, name}` with `kind` one of `session`, `person`,
    /// `nobody` — and there is no top-level `session` field: guessing one
    /// matches every row. `--pile`, `--mine` and `--to` answer the common
    /// questions without filtering JSON by hand.
    ///
    /// ⚠ **THAT IS THE LIST SHAPE, AND `show --json` IS A BIGGER ONE.** A list
    /// row carries no prose — `detailed` is the BOOLEAN standing in for it, so
    /// bodies do not cross the wire to report yes/no. `show --json` adds
    /// `body`, `body_html`, `events`, `restorable`. Reading `detailed` where you
    /// meant `body` yields `true`, and piping that into `edit --body` writes the
    /// string `True` over the prose. **`show <id> --body` prints the body
    /// alone**, and `--previous --body` gives the version before the last edit.
    ///
    /// ⚠ **The service's JSON, reprinted — not rebuilt here**, so it cannot drift
    /// from the API. The human format is the one free to change.
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
        /// Your prompt shows only the first few of these, so this is where the
        /// rest are. Needs no session id — the pile belongs to no conversation.
        #[arg(long, conflicts_with_all = ["all", "mine"])]
        pile: bool,
        /// What you FILED and somebody else is holding — what you handed out.
        ///
        /// The one list that is not about a holder. Your prompt shows your own
        /// work and the pile and deliberately never another conversation's, so
        /// a task you route away is one you stop seeing — this is how to ask
        /// whether any of it is still open.
        ///
        /// Held by anyone but you, the pile included: a task you left for
        /// whoever picks it up is out of your hands too.
        #[arg(long, alias = "handed", conflicts_with_all = ["all", "mine", "pile", "to"])]
        handed_out: bool,
        /// Include finished tasks.
        #[arg(long)]
        done: bool,
        /// What ONE other holder is carrying: a session by name or id,
        /// `pippijn`, or `nobody` for the pile.
        ///
        /// `--assignee` and `--holder` are accepted too. Strictly that holder,
        /// with no pile folded in: the pile is on no holder's plate.
        #[arg(long, conflicts_with_all = ["all", "mine", "pile"], aliases = ["assignee", "holder"])]
        to: Option<To>,
    },
    /// Work on a few things and let the rest go quiet for a while.
    ///
    ///     task focus 849 850 --for 4h
    ///
    /// For those four hours **your prompt** recites only those tasks and counts
    /// the rest. A session pays for every open task it holds on every turn, and
    /// is working on one or two; this is how to say which.
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
    #[command(alias = "history")]
    Show {
        id: TaskRef,
        /// Print the body alone, with no header and no history — for diffing, or
        /// for editing from a faithful copy.
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
    /// refused before anything reaches the service.
    #[command(group(clap::ArgGroup::new("rank").required(true).args(["priority", "unassessed"])))]
    Add {
        /// Optional only so that leaving it out is answered in a sentence rather
        /// than by clap. Refused, never defaulted.
        subject: Option<String>,
        /// The body. `-` reads stdin, which is how a session writes a long one
        /// without fighting shell quoting.
        #[arg(long)]
        body: Option<String>,
        /// Who it is for: `me` (the default — whoever is filing), `pippijn`,
        /// `nobody` for the pile, or a session id.
        #[arg(long)]
        to: Option<To>,
        /// Why this is nobody's — **required by `--to nobody`, and only by it**.
        ///
        /// ⚠ **Filings to the pile were almost always corrected later**, so it is
        /// argued for rather than typed. `--to me` is the default and needs no
        /// argument; this is the one holder that does.
        #[arg(long, value_name = "WHY")]
        spare: Option<String>,
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

        /// Task ids this one UNBLOCKS — the mirror of `--blocked-on`.
        ///
        /// ⚠ **The duplicate check does not compare a filing with the tasks it
        /// names here or in `--blocked-on`**: a blocker resembles what it
        /// unblocks, and the edge proves they are different. Recorded on the
        /// other task once this one is filed.
        #[arg(long = "blocks")]
        blocks: Vec<u64>,
        /// The day it has to be done by: YYYY-MM-DD.
        #[arg(long)]
        due: Option<NaiveDate>,
        /// A concept this tool does not have, kept only to say so.
        ///
        /// ⚠ **A task has had no repo since migration 0004** — the holder is the
        /// whole of an assignment. Accepted only to be refused by name: clap's
        /// `unexpected argument` would leave a session believing it mistyped a
        /// field that exists. What a task touches belongs in the subject.
        #[arg(long, hide = true)]
        repo: Option<String>,
        /// The same.
        #[arg(long, hide = true)]
        project: Option<String>,
        /// Where the subject is NOT given: it is the first positional argument.
        /// Accepted only to be refused by name — `unexpected argument` reads as
        /// a quoting mistake.
        #[arg(long = "subject", hide = true)]
        subject_flag: Option<String>,
        /// File it even though something open or closed already says this.
        ///
        /// ⚠ **For overruling a refusal you have just seen, and only then.** An
        /// open task with the same subject is caught by string equality; the
        /// rest by a model reading the open and closed titles, which takes
        /// seconds — a closed match is sent to `task reopen`. The body you were
        /// filing is still in the command you just ran, so overruling is one
        /// re-run. Passed without a recent refusal of the same subject, the
        /// service refuses the filing. A check that cannot run files the task
        /// and says so.
        #[arg(long)]
        no_duplicate_check: bool,

        /// Ask the check what it would say, and file NOTHING.
        ///
        /// ⚠ **So the check can be tried without filing probe rows** into a
        /// shared tracker, where they would be matched against real work.
        ///
        /// It runs BOTH halves, exactly as a filing does: the string-equality
        /// collision and the model's reading, against open and closed alike.
        #[arg(long, conflicts_with = "no_duplicate_check")]
        check_only: bool,
    },
    /// Mark a task as being worked on.
    Start { id: TaskRef },
    /// Mark a task finished.
    ///
    /// `close` is the same command.
    #[command(alias = "close")]
    Done {
        id: TaskRef,
        /// Where it goes instead. Finishing a task makes the finisher its
        /// holder, so every later list says who did it; this closes and hands
        /// on in one command.
        #[arg(long)]
        to: Option<To>,
        /// What came of it, written above the body before it closes.
        ///
        /// ⚠ **So a task does not close without anybody writing why.** `--reason`
        /// and `--message` are accepted too. `-` reads stdin.
        #[arg(long, aliases = ["reason", "message"])]
        note: Option<String>,
    },
    /// Close a task WITHOUT doing it: overtaken, obsolete, decided against.
    ///
    /// The counterpart to `done`: an obsolete task leaves the list without
    /// anybody being credited with doing it. `--reason` says why.
    Drop {
        id: TaskRef,
        /// Why it is being dropped, written above the body before it closes.
        ///
        /// ⚠ **A dropped task with no reason cannot be told from a decision**: the
        /// status says nothing about why, and a model asked will confidently
        /// invent one. That is why a closed match sends the filer to the TASK.
        #[arg(long, aliases = ["note", "message"])]
        reason: Option<String>,
    },
    /// Put a closed or started task back to open.
    ///
    /// ⚠ **It leaves the holder alone**: whoever last had it is a better guess
    /// than nobody, so reopening something you finished puts it back on your
    /// own plate. `task move <id> nobody` sends it to the pile.
    Reopen { id: TaskRef },
    /// Hand a task over: `me` (this conversation), `pippijn`, `nobody`, or a
    /// session — **by name or by id**, whichever you have.
    ///
    /// A name works because every list prints one. A name that matches nothing,
    /// or matches two conversations, is refused rather than guessed — names
    /// are reused.
    ///
    /// ⚠ **Handing work to a quiet conversation is queueing, not stranding.** A
    /// session never ends — it goes offline and comes back — and its open list
    /// is the work waiting for it. Pick the holder by whose subject it is, not
    /// by who is at a keyboard now.
    ///
    /// `nobody` is for work that genuinely suits whichever conversation is
    /// around next. It is not the safe default for "I am not sure they are
    /// still there" — the pile is a handover channel, not a lost-property
    /// office, and it costs every session's prompt rather than one.
    Move { id: TaskRef, to: To },
    /// Block until somebody else has closed a task, then return.
    ///
    ///     task wait 1350
    ///
    /// ⚠ **Meant to be run in the BACKGROUND, and it is useless in the
    /// foreground.** A session that hits a problem somebody else has to fix
    /// files it, records the edge with `task edit <mine> --blocked-on <theirs>`,
    /// starts this detached, and goes quiet. Claude Code brings a session back
    /// when one of its background commands exits, so this returning IS the
    /// notification — nothing is delivered to the session and no conversation
    /// addresses another.
    ///
    /// The digest drops the `⛔` when the blocker closes, but only renders when
    /// the session takes a turn, and a blocked session is not taking turns.
    ///
    /// It ends when EVERY named task is closed. Exit `0` means they were done;
    /// anything else means carry on at your peril, and the reason is on stderr —
    /// a blocker that was `drop`ped was overtaken, obsolete or decided against,
    /// so the problem this stopped for was not fixed.
    ///
    /// The wait lives in this process, so a restart loses it: the
    /// `--blocked-on` edge and the `⛔` survive, but the automatic wake does
    /// not.
    Wait {
        /// What to wait for. Several means all of them, not the first.
        #[arg(required = true)]
        ids: Vec<TaskRef>,
        /// How long to wait before giving up: `4h`, `90m`, `2h30m`. A bare
        /// number is minutes.
        ///
        /// ⚠ **There is a bound and it cannot be removed**: a wait that can hang
        /// for ever is one nobody finds out about. Giving up wakes the session,
        /// says what is still open, and it can wait again.
        #[arg(long = "for", default_value = "24h")]
        period: String,
    },
    /// Change a task's words.
    ///
    /// `update` and `rank` are the same command.
    #[command(long_about = edit_about(), aliases = ["update", "rank"])]
    Edit {
        id: TaskRef,
        /// The one-line subject. `--title` is accepted too.
        #[arg(long, alias = "title")]
        subject: Option<String>,
        /// `-` reads stdin. REPLACES the whole body — `--prepend` is how to
        /// keep it.
        #[arg(long, conflicts_with_all = ["prepend", "append"])]
        body: Option<String>,
        /// Put text ABOVE the body, keeping every word of it. `-` reads stdin.
        ///
        ///     task edit 42 --prepend "DONE in a2c3ab6 — deployed and verified."
        ///
        /// ⚠ **This is almost always the one you want**: lead with where it stands
        /// and let the history sit under it.
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
        /// mistake than an edit — `detailed` from `--json` taken for the prose
        /// and written over the lot.
        #[arg(long = "replace-body")]
        replace_body: bool,
        /// Rank it: P0 to P4, listed under `task --help`.
        ///
        /// There is no way to UNRANK: a task ranked wrongly is corrected by
        /// ranking it again.
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
        /// A deadline is evidence for a rank, not a substitute for one — until
        /// the last week, when the task sorts and shows as `P0!`. A date earlier
        /// than an open blocker's is refused: it cannot be met.
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
        /// Every conversation there has ever been, including the many never
        /// given anything — where a brand-new conversation's id can be found to
        /// hand it work.
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
    /// How long the commands themselves have been taking, from real use.
    ///
    /// ⚠ **Every row here is a command somebody actually ran**; nothing polls,
    /// so a quiet day reads as quiet.
    Timings {
        /// How far back to look.
        #[arg(long, default_value_t = 7)]
        days: u32,
    },
}

/// The shared secret, from the environment or the file the Mac keeps it in.
///
/// Never on argv: a token in a command line is in every process listing on the
/// machine and in the transcript of the session that typed it.
///
/// Read by THIS BINARY, the CLI — not by the service. The deployment must not
/// supply it: the pod is what the token authenticates *to*, so a copy inside it
/// would be a credential held by its own verifier. `src/main.rs` reads
/// `AGENT_TOKEN` instead.
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
/// `$CLAUDE_CODE_SESSION_ID` is set in every shell Claude Code runs, so a
/// session cannot forget to say who it is or mistype another conversation's
/// id. `$TASKS_SESSION` wins, for a script standing in for one.
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
/// Read from the transcript, so a session need not run `task rename`. See
/// [`tasks::agent_name`].
///
/// Silent on every failure — no `HOME`, no transcript, a changed format: the
/// service keeps whatever name it had.
fn called_now(session: &str) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let projects = std::path::Path::new(&home).join(".claude").join("projects");
    tasks::agent_name::from_projects(&projects, session)
}

/// ⚠ `Clone` so `main` can hold one to report the timing with after handing the
/// command its own. Every field is cheap to clone — `reqwest::Client` is an
/// `Arc` internally and shares its connection pool, so the report reuses the
/// connection the command already opened.
#[derive(Clone)]
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
        self.identified()?;
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
            // ⚠ **A 4xx is the service DECIDING; a 5xx is it falling over**, and
            // the timings count them apart — see `commands::Ended`.
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
        // Propagated, never defaulted: a success whose body will not parse means
        // this CLI and that service disagree about the API, and reporting it as
        // "nothing came back" would send somebody looking at the database.
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

    /// Turn what somebody typed after `move` into a session id.
    ///
    /// The rule and the reason are [`holder::resolve`]'s; this fetches the list
    /// it needs and renders its refusals.
    ///
    /// ⚠ **It refuses rather than falling through to "probably an id"**, with the
    /// known names to hand.
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
            // Every session there is — far longer — only when the short list
            // did not answer.
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
    /// Silent on every failure, including no `HOME`: the write has succeeded,
    /// and missing this only leaves one prompt briefly behind.
    fn forget_cached_digest(&self) {
        let (Some(session), Ok(home)) = (&self.session, std::env::var("HOME")) else {
            return;
        };
        tasks::hook::forget_digest(std::path::Path::new(&home), session);
    }

    /// Refuse before the round trip when this CLI holds half a credential.
    ///
    /// ⚠ Only that one shape: a token with nobody behind it is a guaranteed 401,
    /// and the service's message would not be about this machine. Holding
    /// **neither** is left to the service, which alone knows whether it is
    /// guarded.
    ///
    /// ⚠ **Called from the request path, never from `main`**, so a command that
    /// makes no request — a refused flag name — needs no identity.
    ///
    /// ⚠ **A test that supplies `--session` to get past this tests nothing**:
    /// it passes by hand, where the variable is set, and fails under a
    /// scheduler. The tests clear both variables instead.
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
/// Parsed once, at the argument boundary: anything that is not one of the
/// words is a session, and the destinations are enumerated in one place.
#[derive(Clone, Debug, PartialEq, Eq)]
enum To {
    /// Back in the pile, for whoever picks it up.
    Nobody,
    /// Whoever is running this — for a session, itself.
    ///
    /// ⚠ **`me` is the CALLER, and reading it as the person is the trap.** A
    /// session dealing with a task owns it by default; handing work to the
    /// person is `pippijn`, which says so.
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
/// ⚠ **Outside the spread, not near its middle.** The same prompt at the same
/// settings varies severalfold run to run — what varies is how long the model
/// deliberates, not anything a faster machine shortens — so a bound near the
/// median abandons calls that would have answered. Only the tail pays for the
/// wider one. [`checks`] records every call's elapsed time.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(120);

/// How much a check may deliberate before answering.
///
/// ⚠ **Unbounded, this dominates everything else.** The same prompt runs several
/// times longer and writes an order of magnitude more output tokens to reach the
/// same findings.
///
/// ⚠ **NOT zero, which is what the timings first argue for.** With thinking off,
/// the longest and most tangled bodies come back `DENSE` — *it holds together* —
/// in a couple of seconds. That is a false all-clear on the task that most needs
/// the read, arriving too fast to doubt. Bounded rather than disabled, the same
/// body returns specific findings.
///
/// The filing check takes the same bound on the same reasoning.
const DELIBERATION: &str = "1024";

/// The same, for reading a body rather than a list of titles.
///
/// ⚠ **Size is not what makes one slow.** A very large body can come back faster
/// than a small one: the length of the deliberation is the variable, which is why
/// [`DELIBERATION`] rather than this bound is what made these cheap.
///
/// Wider than [`PATIENCE`] because this call runs AFTER the write, so what waits
/// is a terminal rather than a filing. Not much wider, because a run that reaches
/// this bound has produced nothing at all: every second of it is loss.
const READING: std::time::Duration = std::time::Duration::from_secs(90);

/// Every open task, as the id and title a duplicate would be spotted by.
///
/// ⚠ **Every open task, not this session's**: the duplicate that matters is the
/// one another conversation filed, which nobody else can see.
///
/// ⚠ **Read before the POST**, so both halves of the check refuse while there
/// is still nothing to undo.
async fn open_now(client: &Client) -> Result<Vec<(u64, String)>> {
    let query = list_query(true, false, false, false, false, None, None)?;
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

/// Every closed task worth reading, as the block that gets cached.
///
/// ⚠ **Failure here must not cost the filing**: the check falls back to the
/// open list, so the error is swallowed and the list comes back empty.
///
/// ⚠ **Filtered on the way out, and the count is reported** — see
/// [`duplicates::worth_reading`]. A corpus that silently shrinks reads as
/// covering more than it does.
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

/// What a model names, open or closed, as already saying what is about to be
/// filed.
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
    // The closed list is deliberately NOT counted: `input_chars` is what this
    // filing put in front of the model, and the cached prefix is the same bytes
    // for every filing.
    let input_chars = asked.chars().count().min(u32::MAX as usize) as u32;
    let prefix = (!settled.is_empty()).then(|| duplicates::settled_block(settled));
    let (said, elapsed_ms) = ask_with(&asked, prefix.as_deref(), PATIENCE).await;
    // Against both lists: an id off either one is a real task, and `split` is
    // what decides which of the two things this filing is told.
    let known: Vec<(u64, String)> = corpus
        .iter()
        .cloned()
        .chain(settled.iter().map(|t| (t.id, t.subject.clone())))
        .collect();
    let found = match &said {
        Ok(words) => duplicates::parse(words, &known),
        Err(_) => Vec::new(),
    };
    // ⚠ **Only when it actually named something.** The key is what licenses a
    // later `--no-duplicate-check` for this subject, so recording it on a check
    // that passed would hand out a licence nobody was refused.
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
            // A filing check judges a title against a list, and the caller is
            // holding the body it was about to file — there is no task yet to
            // keep anything on.
            said: None,
        },
    )
    .await;
    // The failure is still the caller's to print: it says a filing went
    // unchecked, which a table row does not tell the session in front of it.
    said?;
    Ok(found)
}

/// Report one run, and never let reporting it cost anything.
///
/// Silent on every failure, for the reason [`tasks::tasks::commands`] gives:
/// this runs after the call it describes, so there is nothing left to protect.
async fn recorded(client: &Client, run: checks::Run) {
    let req = client
        .request(reqwest::Method::POST, "/api/checks")
        .json(&run);
    let _ = client.send(req).await;
}

/// Whether this process ever waited for a model.
///
/// ⚠ **Set where the waiting happens**, which both checks funnel through, so a
/// timeout counts: a run that waited and got nothing belongs in the slow
/// population. A process runs one command, so a flag is all the state needed.
static WAITED_FOR_A_MODEL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Put the question to a one-shot session and leave nothing behind.
///
/// ⚠ **Every call is a conversation, and leaves a transcript.** Left alone they
/// accumulate without bound, so the id is named here and the file goes the
/// moment the answer is in hand — on the failing paths too.
///
/// ⚠ **`prefix` is where a cached prefix goes, and it must not vary per call** —
/// see [`duplicates::settled_block`].
///
/// ⚠ **On a file, not in the argument list**, for the reason [`call`] gives
/// about the prompt. The file goes with the transcript.
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
    // Before `discard`, which is a file removal on the same path and no part of
    // what was being measured.
    let took = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    discard(&named);
    if let Some(path) = carried {
        let _ = std::fs::remove_file(path);
    }
    (said, took)
}

/// The call itself, up to the words that came back.
///
/// ⚠ **On stdin, not in the argument list.** The prompt carries every open
/// title, and an argument that size is at the mercy of a shell's limits and of
/// anything that logs a command line.
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
        // The one setting that decides what a check costs. See [`DELIBERATION`].
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
/// ⚠ **One deliberate difference: a pile row says who filed it.** The digest
/// must not — every session pays for it every turn — but a list is fetched by
/// somebody asking what to pick up.
fn line(task: &Task) -> String {
    let marker = task.status.marker();
    // Before the subject, so ranks scan down the left edge, padded so nothing
    // shifts between ranked and unranked lines. `!` marks a rank a deadline
    // raised, so the order never reads as random.
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
    // ⚠ **On the first line `task show` prints**, so a reader piping to `head`
    // gets the count wherever it cuts.
    match task.body_lines {
        0 => {}
        1 => out.push_str("  [1 line]"),
        n => out.push_str(&format!("  [{n} lines]")),
    }
    // The digest's marker, from the same field, so the two agree.
    if let Some(chars) = task.sprawl_chars {
        out.push_str(&format!("  [sprawl {}]", tasks::digest::thousands(chars)));
    }
    // `label()` is the one spelling of name-else-id.
    if task.assignee.kind != AssigneeKind::Nobody {
        out.push_str(&format!("  ({})", task.assignee.label()));
    } else if let Some(from) = &task.filed_by {
        out.push_str(&format!("  (from {from})"));
    }
    out
}

/// Print a service answer: verbatim when `--json` was asked for, otherwise
/// however the caller draws it.
///
/// One helper, so no command can quietly ignore the flag and hand a script the
/// human format as JSON.
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

impl Command {
    /// The name this command is recorded and grouped under.
    ///
    /// ⚠ **Exhaustive on purpose — no `_ =>` arm.** `verb` is the trend key, and
    /// a catch-all would merge two commands' histories.
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

/// Report what a command did, and never let reporting it cost anything.
///
/// ⚠ **After the work and after the printing**, so the round trip is not in what
/// anybody waits for, and silent on every failure — `commands` carries why.
///
/// ⚠ **`timings` and `checks` are not recorded.** Reading the measurements is
/// not use of the tool, and recording it would show a command whose whole
/// population is people looking at it.
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
        // ⚠ **Always `Some`**: `None` on the wire means an older client that
        // could not say.
        waited_for_a_model: Some(WAITED_FOR_A_MODEL.load(std::sync::atomic::Ordering::Relaxed)),
    };
    let req = client
        .request(reqwest::Method::POST, "/api/commands")
        .json(&run);
    // The service answers with the numbers when this caller is the one asked to
    // carry them out, and with nothing the rest of the time.
    let Ok(Some(answer)) = client.send(req).await else {
        return;
    };
    if let Some(report) = answer.get("report")
        && let Err(why) = fleetwatch::send(&client.http, report).await
    {
        // Same rule as the status line above: it may not cost the command, and
        // it may not disappear.
        eprintln!("(the timings did not reach fleetwatch: {why:#})");
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // ⚠ First statement in the process: what a session waits for includes
    // argument parsing and building the client.
    let started = std::time::Instant::now();
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
    // ⚠ **Printed here rather than returned**: returning prints anyhow's whole
    // chain. `commands::said` decides which endings deserve one.
    if let Err(why) = &done {
        eprintln!("{}", commands::said(why));
        std::process::exit(1);
    }
    Ok(())
}

/// Everything the CLI does, so that `main` can time all of it.
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
            // ⚠ Resolved through the SAME path `move` uses, so `--to hardware`
            // and `move <id> hardware` cannot disagree about who that is.
            let holder = match to {
                Some(to) => Some(client.resolve(to).await?),
                None => None,
            };
            let me = client.me().ok();
            let asked = holder.as_ref().map(|to| match to {
                To::Nobody => selection::Holder::Nobody,
                To::Person => selection::Holder::Person("pippijn"),
                To::Me | To::Session(_) => match to {
                    To::Session(id) => selection::Holder::Session(id),
                    // `--to me` is `--mine` said another way, and a session that
                    // cannot name itself has already failed `identified`.
                    _ => selection::Holder::Session(me.unwrap_or_default()),
                },
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
            // ⚠ **Decoded here, outside the closure, so it can use `?`**: a list
            // this CLI cannot read is an error, not a plausible wrong row.
            let shown: Vec<Task> = serde_json::from_value(tasks.clone())
                .context("the service answered with a list this CLI could not read")?;
            emit(cli.json, &tasks, || {
                if shown.is_empty() {
                    // Which question came back empty: an empty pile and an empty
                    // plate mean different things.
                    println!(
                        "{}",
                        if pile {
                            "the pile is empty"
                        } else if handed_out {
                            // Good news rather than an empty plate: everything
                            // this session routed away has been closed.
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
                // The header says when this stopped being the task — the edit
                // `task undo` reverses.
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
                // Exactly the stored markdown and nothing else — no trailing
                // newline added or removed — so it can be diffed against
                // another copy without the diff being about this program.
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
                // ⚠ **Between the body and the history**: it is about the body,
                // not something that happened. Printed on every `task show`
                // until an edit makes the body smaller.
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
            // Read by clap's group, not here: `--unassessed` is the absence of
            // `--priority` once one of the two is known to have been given.
            unassessed: _,
            blocked_on,
            blocks,
            due,
            repo,
            project,
            subject_flag,
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
            // ⚠ **Before the duplicate check**, which would spend a model call
            // on a filing the service then refuses. The service owns the rule
            // (`repo::spare_note`); this is a faster no. Blank-trimmed as the
            // service trims, or the two would disagree about `--spare ""`.
            let reason = spare
                .as_deref()
                .map(str::trim)
                .filter(|why| !why.is_empty());
            match holder::pile_verdict(matches!(to, Some(To::Nobody)), reason.is_some()) {
                holder::PileVerdict::Fits => {}
                refused => return Err(commands::declined(refused.said_to_cli())),
            }
            client.writing()?;
            // Two questions, both answered before the POST and both refusing:
            // does something open carry this exact subject, and does a model
            // see the same problem in different words. `--check-only` runs both
            // and stops before anything is sent.
            //
            // ⚠ **A list that could not be read costs a note, never a filing.**
            let corpus = match no_duplicate_check {
                true => Vec::new(),
                false => open_now(&client).await.unwrap_or_else(|why| {
                    eprintln!("(duplicate check did not run: {why:#})");
                    Vec::new()
                }),
            };
            if let Some(already) = duplicates::same_subject(&subject, &corpus) {
                return Err(commands::declined(duplicates::collision(already)));
            }
            // ⚠ **The model runs BEFORE the POST**: a refusal after the fact is
            // not one. A check that could not run files the task (see
            // `duplicates`).
            //
            // ⚠ **Narrowed after the collision check, never before**: string
            // equality runs over every open title; this narrows only what the
            // model judges.
            let candidates = duplicates::edged(&corpus, &blocked_on, &blocks);
            // The closed list is never narrowed by the edges: nothing can wait
            // for a task that is over.
            let (settled, unread) = match no_duplicate_check {
                true => (Vec::new(), 0),
                false => settled_now(&client).await,
            };
            if !no_duplicate_check {
                match already_filed(&client, &candidates, &settled, &subject).await {
                    Ok(found) if !found.is_empty() => {
                        let (open, over) = duplicates::split(&found, &settled);
                        // ⚠ **Both halves refuse, and open wins when one answer
                        // names both.** A live task is the stronger remedy:
                        // folding into work still going beats reopening work
                        // that stopped, and the closed one is still reachable.
                        //
                        // ⚠ **`declined`, never `bail!`**, or the commonest
                        // refusal counts as a failure and prints its chain.
                        let said = match open.is_empty() {
                            false => duplicates::refusal(&open),
                            true => duplicates::reopen_instead(&over, settled.len(), unread),
                        };
                        return Err(commands::declined(said));
                    }
                    Ok(_) => {}
                    Err(why) => eprintln!("(duplicate check did not run: {why:#})"),
                }
            }
            // Reached only when nothing was named: every match has already
            // returned as its refusal, on the exit code a real filing gets.
            if check_only {
                println!("NONE — nothing open or closed was named");
                return Ok(());
            }
            let mut payload = json!({
                "subject": subject,
                "body": raw.as_deref().map(body).transpose()?.unwrap_or_default(),
                // ⚠ **Said plainly**: the service must tell "skipped" from an
                // older client that never mentioned it.
                "checked": !no_duplicate_check,
            });
            // Always present, null when unassessed: the service refuses a
            // filing that omits it. clap's group guarantees one of the two.
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
            // Sent as given: the pairing was refused above, and the service
            // refuses it again from the type.
            if let Some(spare) = spare {
                payload["spare"] = json!(spare);
            }
            let req = client
                .request(reqwest::Method::POST, "/api/tasks")
                .json(&payload);
            let task = client.send(req).await?.context("no task came back")?;
            let shown: Task = serde_json::from_value(task.clone())
                .context("the service answered with a task this CLI could not read")?;
            emit(cli.json, &task, || println!("{}", line(&shown)));
            // ⚠ **After the POST, because the edge needs this task's id.** Recorded
            // on the OTHER task: `blocked_on` belongs to the thing that waits.
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
            // ⚠ **The note lands BEFORE the close**, so a task is never finished
            // and silent about why.
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
            // Bounded before the first request, so a mistyped period is a
            // refusal rather than something discovered a day later.
            let bound = focus::parse(&period)?
                .to_std()
                .context("a wait cannot be negative")?;
            let asked: Vec<u64> = ids.iter().map(TaskRef::id).collect();
            // A task that does not exist is refused on the FIRST pass, because
            // `statuses` errors, rather than polled for a day.
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
                // Never sleep past the bound.
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
            // After the write: this is advice about prose, never a refusal.
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
            emit(cli.json, &rows, || {
                if runs.is_empty() {
                    println!("nothing recorded in the last {days} days");
                    return;
                }
                for line in checks::tally(&runs) {
                    println!("{}", tallied(&line));
                }
            });
        }
        Command::Sessions { all } => {
            // Two questions, two routes: `/api/holders` is who is carrying what;
            // `/api/sessions` is every conversation that ever ran, asked for
            // only to find a new session's id.
            let path = if all { "/api/sessions" } else { "/api/holders" };
            let req = client.request(reqwest::Method::GET, path);
            let rows = client.send(req).await?.unwrap_or(json!([]));
            emit(cli.json, &rows, || {
                for holder in rows.as_array().cloned().unwrap_or_default() {
                    // `open/total`, not `open`: see `sessions::Holder`. The id
                    // stays because it is the handle for `task move`. `--all`
                    // rows carry no total, so they print the open count alone.
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
            // ⚠ Refused rather than accepted-and-reverted: every request carries
            // the name Claude Code uses, so a different rename would be undone
            // by the next command. Say where the lever is.
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

/// What the service currently says about each of these tasks.
///
/// ⚠ **One request per task, deliberately**: the list endpoint answers by
/// holder, and a wait's blockers are held by whoever is fixing them. At a poll
/// every few seconds, the round trips cost nothing.
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

/// The task as it stood before its last edit, or why there is no such thing.
///
/// ⚠ **The service answers 404 for two states** — no such task, and nothing
/// overwritten — so the message names both, or a mistyped id and an unedited
/// task would read as the same mistake.
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
/// `null` is the ordinary answer, an absence rather than a failure.
fn parse_focus(value: &Value) -> Option<tasks::tasks::focus::Focus> {
    serde_json::from_value(value.clone()).ok()
}

/// A focus as one line: what, until when, and how much is left of it.
///
/// ⚠ **The countdown is here and never in the digest**: this runs the moment
/// somebody types the command; the digest is cached and read later.
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

/// Record that `blocked` is waiting for `filed`.
///
/// ⚠ **Read, extend, write — never replace.** `blocked_on` is the whole set on
/// the wire, so sending one id would silently drop every other edge that task
/// already declared.
///
/// ⚠ **A failure here does not fail the filing.** The task exists by now; a
/// missing edge is a note on stderr, not a lost body.
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

/// Put the outcome above the body, before the task closes.
///
/// ⚠ **`--prepend`, never `--body`**: the note is the conclusion and the body the
/// history that earned it.
///
/// ⚠ **The density read deliberately does not run here**: a task being closed
/// is not about to be rewritten, and the read would make `--note` slower than
/// the two commands it replaces.
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

/// Change a task, named either way.
///
/// ⚠ **A write that moved nothing says so**, or it prints a line identical to
/// the one a real change produces. `task start` on a task already `doing` in the
/// pile claims nobody and looks like it worked; so does a rename to a blank
/// name, and closing into the pile. Each is only findable by reproducing it.
///
/// It is a note, not an error: a no-op is often the right answer — starting a
/// task already yours is meant to be quiet — and a non-zero exit would turn a
/// silent success into a spurious failure. The service reports which
/// `task_events` it wrote; empty means none.
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
        println!("{}", line(&shown));
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
/// ⚠ **Every failure here is silence**: the edit has landed, so a missing
/// `claude`, a timeout or an unreadable body is no reason to spend a session's
/// attention on the checker.
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
            // A density read has no subject to license anything with.
            subject_key: None,
            // ⚠ **What the MODEL said, not what this prints**: the wrapper is
            // addressed to whoever ran this edit, not to the next reader.
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

/// What an edit landed on, said back to whoever made it.
///
/// See [`Replaced`](tasks::tasks::types::Replaced) for why this tells rather
/// than refuses. The undo comes with it — this is when the remedy is wanted.
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

/// One kind of check, as a line.
///
/// Zero counts are dropped, but a timeout is printed even when it is all that
/// happened: `quiet` and `timeout` both end in silence, and only one ran.
fn tallied(line: &checks::Tally) -> String {
    let kind = match line.kind {
        checks::Kind::Filing => "filing",
        checks::Kind::Density => "density",
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

/// One command's line.
///
/// ⚠ **The run count comes first because it is the weight**, as in
/// [`commands::tally`].
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
