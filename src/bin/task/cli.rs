//! The command line: flags, subcommands and their `--help` text.

use super::*;

/// What `task --help` adds: the levels come from [`Priority::gloss`], never a second copy.
pub(crate) fn long_about() -> String {
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
pub(crate) fn edit_about() -> String {
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
pub(crate) struct Cli {
    /// Base URL of the service. Defaults to $TASKS_URL, then the VPN name.
    #[arg(long, global = true)]
    pub(crate) url: Option<String>,
    /// This conversation's session id. Defaults to $TASKS_SESSION, then $CLAUDE_CODE_SESSION_ID.
    #[arg(long, global = true)]
    pub(crate) session: Option<String>,
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
    pub(crate) json: bool,
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
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

impl Command {
    /// The name a command is recorded under. ⚠ Exhaustive: a catch-all would merge two
    /// commands' histories.
    pub(crate) fn verb(&self) -> &'static str {
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
