//! Whether a task about to be filed is already on the list.
//!
//! Several conversations file here and none can see the others' lists, so the
//! same problem arrives twice in different words. A string comparison cannot
//! catch that — two spellings of one problem often share no words — so a model
//! is asked, before the filing rather than after.
//!
//! Every arm refuses and `--no-duplicate-check` passes all of them: a refusal
//! costs one re-run, since the caller still holds the body it tried to file,
//! where a duplicate costs somebody's attention twice.
//!
//! ⚠ **A check that could NOT run files the task and says so.** A missing
//! `claude`, a timeout, an unreadable answer. Only a model that actually names
//! something refuses; a session that cannot write things down is the worst
//! outcome available here.
//!
//! [`same_subject`] is string equality and has no error rate. [`prompt`] is the
//! guess, and says so. Closed tasks are read too and refuse as well, but their
//! remedy is `task reopen` rather than a re-run — see [`reopen_instead`].

/// The open task whose subject is already the one being filed, if there is one.
///
/// Case and surrounding space are ignored: neither distinguishes two pieces of
/// work, and both are what a second attempt at one filing varies by.
pub fn same_subject(subject: &str, corpus: &[(u64, String)]) -> Option<u64> {
    let want = subject.trim();
    corpus
        .iter()
        .find(|(_, open)| open.trim().eq_ignore_ascii_case(want))
        .map(|(id, _)| *id)
}

/// What a refused filing says.
///
/// ⚠ **Names all three ways out.** A collision is usually an *update* to the
/// task that exists, which is `task edit`; the override is spelled in full so
/// nobody has to find it in `--help` while holding a body on stdin.
pub fn collision(already: u64) -> String {
    format!(
        "NOT FILED — #{already} is already open with this exact subject. \
         `task show {already}` to read it, `task edit {already}` if this is an update to it, \
         or re-run with --no-duplicate-check if they really are two tasks."
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub id: u64,
    /// Its one clause about what the two share. Printed as given.
    pub why: String,
}

/// A fourth line is not a fourth candidate worth opening — by then the answer
/// is a list of everything in that area.
const MOST: usize = 3;

/// The list a model is shown, without either end of an edge the filing declares.
///
/// ⚠ **A blocker resembles what it unblocks by construction**, so without this
/// the check condemns a task for the relationship that proves it is a different
/// one. No better prompt fixes that, because the reading is not wrong.
///
/// ⚠ **[`same_subject`] still runs against the WHOLE list** — an identical
/// title is one task whatever edge was declared.
///
/// Filtered here because a new task's edges reach the service only in the POST
/// that follows this check.
pub fn edged(corpus: &[(u64, String)], waiting_on: &[u64], unblocks: &[u64]) -> Vec<(u64, String)> {
    corpus
        .iter()
        .filter(|(id, _)| !waiting_on.contains(id) && !unblocks.contains(id))
        .cloned()
        .collect()
}

/// What the model is asked.
///
/// ⚠ **The negative half of the instruction does most of the work.** Asked
/// plainly whether two tasks are "similar", a model answers about subject
/// matter and returns everything in the same repository. Naming what is *not* a
/// duplicate is what makes the answer usable.
pub fn prompt(subject: &str, corpus: &[(u64, String)], settled: bool) -> String {
    let lines: String = corpus
        .iter()
        .map(|(id, subject)| format!("{id} | {subject}\n"))
        .collect();
    // Named rather than inlined to keep the two blocks visibly apart: this one
    // varies every filing and the one it points at does not.
    let also = if settled {
        "Your instructions also carry every CLOSED task — finished or abandoned. \
         Those count: a task already done should be reopened, not filed again.\n\n"
    } else {
        ""
    };
    format!(
        "A session is about to file this task into a shared tracker:\n\n  {subject}\n\n\
         Below is every task already open, one per line, as `id | title`.\n\n\
         {lines}\n\
         {also}\
         Does the new task describe the SAME underlying problem as one already there?\n\
         Same repo, same area, or same technology is NOT the same problem. Two different \
         bugs in one file are NOT duplicates. A task that would be closed as \"already \
         filed\" IS a duplicate.\n\n\
         Answer with one line per match, at most {MOST}:\n\
         #<id> -- <one clause saying what they share>\n\
         If nothing matches, answer with exactly: NONE\n\
         No preamble, no markdown."
    )
}

/// The matches in a model's answer.
///
/// ⚠ **Unparseable lines are dropped**, or a model that explains itself puts a
/// paragraph into a refusal.
///
/// ⚠ **An id not on the list it was given is discarded.** A hallucinated number
/// must not be able to block work, and the corpus is the only thing that says
/// which numbers were real.
pub fn parse(said: &str, corpus: &[(u64, String)]) -> Vec<Match> {
    let mut found: Vec<Match> = Vec::new();
    for line in said.lines() {
        let line = line.trim();
        // A model that says NONE and then explains why has said the useful
        // part first.
        if line.eq_ignore_ascii_case("NONE") {
            break;
        }
        let Some(one) = one(line) else { continue };
        if !corpus.iter().any(|(id, _)| *id == one.id) {
            continue;
        }
        if found.iter().any(|seen| seen.id == one.id) {
            continue;
        }
        found.push(one);
        if found.len() == MOST {
            break;
        }
    }
    found
}

/// A numbered list's number, removed — but only where something else is
/// plainly the id.
///
/// ⚠ **A leading `1.` reads as an id and low-numbered tasks are real.**
/// Stripping every leading `<digits>.` would be worse, since a bare number is a
/// plausible way to name a task, so it goes only when a `#` id follows.
fn ordinal(line: &str) -> &str {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return line;
    }
    let rest = &line[digits..];
    let Some(rest) = rest.strip_prefix(['.', ')']) else {
        return line;
    };
    let rest = rest.trim_start();
    if rest.starts_with('#') { rest } else { line }
}

/// Tolerant of the wrappers a model adds despite being asked not to. Not
/// tolerant of a missing id, which is what separates an answer from a sentence
/// about the answer.
fn one(line: &str) -> Option<Match> {
    // Emphasis goes because this is printed as plain text, where it arrives as
    // punctuation rather than as formatting.
    let line: String = line.chars().filter(|c| !"`*_".contains(*c)).collect();
    let line = line.trim().trim_start_matches(['-', '•', '*']).trim();
    let line = ordinal(line);
    let line = line.strip_prefix('#').unwrap_or(line);
    let digits: String = line.chars().take_while(char::is_ascii_digit).collect();
    let id: u64 = digits.parse().ok()?;
    let rest = line[digits.len()..].trim();
    let why = rest
        .trim_start_matches(['-', '—', '–', ':'])
        .trim()
        .to_string();
    // An id with nothing after it is a number, not a finding: a bare id makes
    // a reader open a task to learn whether it was worth opening.
    (!why.is_empty()).then_some(Match { id, why })
}

/// What a refused filing says.
///
/// ⚠ **It says who is talking.** Every other line this CLI prints is read off
/// the service; this one is a model's reading of titles and has to admit it. A
/// caller told *this is a duplicate* checks nothing; one told *a model thinks
/// so, here is what it matched* opens the task.
///
/// ⚠ **The override is spelled out in full.** The caller still holds the text
/// it tried to file, so the remedy is one re-run — saying so is what keeps a
/// false positive cheap.
pub fn refusal(found: &[Match]) -> String {
    let mut out = String::new();
    for one in found {
        out.push_str(&format!("  #{:<4} {}\n", one.id, one.why));
    }
    out.push_str(
        "NOT FILED — a model reading the open titles says this is already one of them. \
         `task show <id>` to check one, or re-run the same command with \
         --no-duplicate-check if this really is different work.",
    );
    out
}

/// A closed task, as the check reads it.
///
/// ⚠ **`dropped` and `done` are kept apart** so the refusal can say which: a
/// done task whose bug came back is reopened, while an abandoned one being
/// filed again is a decision made twice, and its reason is in the task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    pub id: u64,
    pub subject: String,
    /// Closed without being done — see [`Status::Dropped`].
    ///
    /// [`Status::Dropped`]: crate::tasks::types::Status::Dropped
    pub dropped: bool,
}

/// Whether a closed row is a description of work at all.
///
/// Much of the dropped pile is this tool testing itself, including the
/// paraphrase pairs written to verify this very check.
///
/// ⚠ **A property of the row, never a list of ids** — a hard-coded list rots at
/// the next probe.
///
/// ⚠ **Closing quickly is NOT the signal**: some valuable closed rows were
/// dropped within a minute and carry a complete plan. What separates the
/// fixtures is that they say nothing — a filing with no body.
pub fn worth_reading(detailed: bool) -> bool {
    detailed
}

/// The closed corpus, as the block that goes in front of the question.
///
/// ⚠ **Nothing that varies per filing may be appended to this string.** It goes
/// where a cached prefix goes, and a cache block ends where the varying text
/// begins — put the subject below this list in one message and every call
/// rewrites the whole corpus without reading any of it back. The open list and
/// the subject belong in [`prompt`].
pub fn settled_block(corpus: &[Settled]) -> String {
    let lines: String = corpus
        .iter()
        .map(|task| {
            let status = if task.dropped { "dropped" } else { "done" };
            format!("{} | {status} | {}\n", task.id, task.subject)
        })
        .collect();
    format!(
        "You judge whether a task about to be filed already exists in a shared tracker.\n\n\
         Below is every CLOSED task, one per line, as `id | status | title`. `done` means \
         the work was finished; `dropped` means it was abandoned, overtaken, or decided \
         against.\n\n\
         {lines}\n\
         A new filing that describes work already on this list is not new work. Name it by \
         its id, exactly as you would name an open one."
    )
}

/// Which list each match came off. [`parse`] has already dropped ids on
/// neither.
pub fn split(found: &[Match], settled: &[Settled]) -> (Vec<Match>, Vec<(Match, Settled)>) {
    let mut open = Vec::new();
    let mut over = Vec::new();
    for one in found {
        match settled.iter().find(|task| task.id == one.id) {
            Some(task) => over.push((one.clone(), task.clone())),
            None => open.push(one.clone()),
        }
    }
    (open, over)
}

/// What a filing is told when it resembles something already closed.
///
/// ⚠ **The remedy is why this is not [`refusal`] with a different list.** An
/// open twin is folded into or re-run past; a closed one is `task reopen`, and
/// a session not told so files anyway.
///
/// ⚠ **The verdict is the LAST line, corpus counts and all**, because sessions
/// pipe this to `tail -3`. See the `what_survives_the_tail` tests.
pub fn reopen_instead(found: &[(Match, Settled)], read: usize, unread: usize) -> String {
    let mut out = String::new();
    for (one, task) in found {
        let status = if task.dropped {
            "dropped, and the reason is in the task rather than in its status"
        } else {
            "already done"
        };
        out.push_str(&format!("  #{:<4} {} — {status}\n", one.id, one.why));
    }
    out.push_str(&format!(
        "NOT FILED — a model reading the closed titles says this work already exists. \
         `task reopen <id>` if it is the same work and carry on in that task, or re-run the \
         same command with --no-duplicate-check if it really is different \
         (read against {read} closed tasks; {unread} skipped as having no body)."
    ));
    out
}
