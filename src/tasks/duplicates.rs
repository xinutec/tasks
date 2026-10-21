//! Whether a task about to be filed is already on the list. Conversations
//! can't see each other's lists, and two spellings of one problem often share
//! no words, so a model is asked before filing. A refusal costs one re-run (the
//! caller still holds the body); a duplicate costs attention twice.
//!
//! ⚠ A check that could not run (no `claude`, a timeout, an unreadable answer)
//! files the task and says so: only a model that names something refuses.
//! [`same_subject`] is exact; [`prompt`] is the guess. A closed match's remedy
//! is `task reopen` ([`reopen_instead`]).

/// The open task with this exact subject, ignoring case and surrounding space
/// (what a second attempt at one filing varies by).
pub fn same_subject(subject: &str, corpus: &[(u64, String)]) -> Option<u64> {
    let want = subject.trim();
    corpus
        .iter()
        .find(|(_, open)| open.trim().eq_ignore_ascii_case(want))
        .map(|(id, _)| *id)
}

/// Names all three ways out: usually it is an update (`task edit`), and the
/// override is spelled in full.
pub fn collision(already: u64) -> String {
    format!(
        "NOT FILED — #{already} is already open with this exact subject. \
         `task show {already}` to read it, `task edit {already}` if this is an update to it, \
         or re-run with --not-duplicate-of {already} if they really are two tasks."
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub id: u64,
    /// Its one clause about what the two share, printed as given.
    pub why: String,
}

/// Past three, the answer is everything in the area.
const MOST: usize = 3;

/// The open list without either end of an edge the filing declares: a blocker
/// resembles what it unblocks by construction, so the model would condemn the
/// very relationship that proves they differ. [`same_subject`] still sees all.
pub fn edged(corpus: &[(u64, String)], waiting_on: &[u64], unblocks: &[u64]) -> Vec<(u64, String)> {
    corpus
        .iter()
        .filter(|(id, _)| !waiting_on.contains(id) && !unblocks.contains(id))
        .cloned()
        .collect()
}

/// What the model is asked. Saying what is not a duplicate does most of the
/// work: asked about "similar", a model returns everything in the repository.
pub fn prompt(subject: &str, corpus: &[(u64, String)], settled: bool) -> String {
    let lines: String = corpus
        .iter()
        .map(|(id, subject)| format!("{id} | {subject}\n"))
        .collect();
    // Kept apart from the cached block: this part varies every filing.
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

/// The matches in a model's answer. Unparseable lines are dropped, and so is
/// any id not on the list it was shown: a hallucinated number must not block
/// work.
pub fn parse(said: &str, corpus: &[(u64, String)]) -> Vec<Match> {
    let mut found: Vec<Match> = Vec::new();
    for line in said.lines() {
        let line = line.trim();
        // NONE then an explanation is still NONE.
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

/// A list's `1.` removed, but only before a `#` id: low-numbered tasks are
/// real, and a bare number may be one.
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

/// Tolerates the wrappers a model adds; a line without an id is not an answer.
fn one(line: &str) -> Option<Match> {
    // Emphasis is stripped: printed as plain text it is punctuation.
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
    // An id with no reason is not a finding.
    (!why.is_empty()).then_some(Match { id, why })
}

/// What a refused filing says. It admits a model is talking (a caller told *a
/// model thinks so* opens the task; one told *this is a duplicate* checks
/// nothing), spells the override, and when the filer holds the match, says to
/// hand that task on instead of filing a second.
pub fn refusal(found: &[Match], open: &[(u64, String)], mine: &[u64]) -> String {
    let mut out = String::new();
    for one in found {
        out.push_str(&format!("  #{:<4} {}\n", one.id, one.why));
    }
    let named = found
        .iter()
        .map(|one| match open.iter().find(|(id, _)| *id == one.id) {
            Some((id, subject)) => format!("#{id} ({subject})"),
            None => format!("#{}", one.id),
        });
    let held: Vec<u64> = found
        .iter()
        .map(|one| one.id)
        .filter(|id| mine.contains(id))
        .collect();
    let hand_on = match held.as_slice() {
        [] => String::new(),
        ids => {
            let named: Vec<String> = ids.iter().map(|id| format!("#{id}")).collect();
            let is = if ids.len() == 1 { "is" } else { "are" };
            format!(
                " {} {is} yours: if this is what is left of it, hand it on rather than \
                 filing a second task — `task edit {} --to <who> --body -` with what is left.",
                named.join(" and "),
                only(ids.iter().copied()),
            )
        }
    };
    out.push_str(&format!(
        "NOT FILED — a model reading the open titles says this is already {}. \
         `task show {}` to check, or re-run the same command with \
         --not-duplicate-of {} if this really is different work.{hand_on}",
        either(named),
        only(found.iter().map(|one| one.id)),
        ids(found.iter().map(|one| one.id)),
    ));
    out
}

/// The matches as one phrase: `#1 (…)`, or `#1 (…) or #2 (…)`.
fn either(named: impl Iterator<Item = String>) -> String {
    named.collect::<Vec<_>>().join(" or ")
}

/// Every id, as `--not-duplicate-of` takes them.
fn ids(all: impl Iterator<Item = u64>) -> String {
    all.map(|id| id.to_string()).collect::<Vec<_>>().join(",")
}

/// The id a remedy can be spelled with, when there is exactly one.
fn only(mut ids: impl ExactSizeIterator<Item = u64>) -> String {
    match (ids.len(), ids.next()) {
        (1, Some(id)) => id.to_string(),
        _ => "<id>".to_string(),
    }
}

/// A closed task as the check reads it. Done and dropped differ: a done bug
/// that came back is reopened, while refiling a dropped one repeats a decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    pub id: u64,
    pub subject: String,
    /// Closed without being done ([`Status::Dropped`]).
    ///
    /// [`Status::Dropped`]: crate::tasks::types::Status::Dropped
    pub dropped: bool,
}

/// Whether a closed row describes work at all. Many dropped rows are this
/// check's own test fixtures, and what marks them is an empty body, not a quick
/// close (some real plans were dropped within a minute) and never a list of ids.
pub fn worth_reading(detailed: bool) -> bool {
    detailed
}

/// The closed corpus, the cached block before the question.
///
/// ⚠ Nothing per-filing may be appended: a cache block ends where varying text
/// begins, and the whole corpus would be rewritten every call. The open list
/// and subject go in [`prompt`].
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

/// Which list each match came off; [`parse`] already dropped ids on neither.
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

/// What a filing is told when it resembles a closed task: the remedy is
/// `task reopen`, which a session not told so never does. The verdict, ids and
/// counts included, is the last line: sessions read the tail.
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
    let named = found.iter().map(|(one, task)| {
        let status = if task.dropped { "dropped" } else { "done" };
        format!("#{} ({status}: {})", one.id, task.subject)
    });
    out.push_str(&format!(
        "NOT FILED — a model reading the closed titles says this work is {}. \
         `task reopen {}` if it is the same work and carry on in that task, or re-run the \
         same command with --not-duplicate-of {} if it really is different \
         (read against {read} closed tasks; {unread} skipped as having no body).",
        either(named),
        only(found.iter().map(|(one, _)| one.id)),
        ids(found.iter().map(|(one, _)| one.id)),
    ));
    out
}
