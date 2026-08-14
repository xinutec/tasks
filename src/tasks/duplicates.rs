//! Whether a task about to be filed is already on the list.
//!
//! Several conversations file into this tracker and none of them can see the
//! others' lists, so the same problem arrives twice in different words. The
//! service cannot catch it — `subject` has no uniqueness a string comparison
//! could enforce, and the two spellings of one problem share no words: #255 was
//! *"NOT an outage: the cache holds 995 routes ... the cron has simply never
//! run"* and #760, filed later by a different session, was *"health-bus-refresh:
//! the 'defined in no repo' claim was a search artefact — but it IS suspended
//! and has never run"*. Nothing but a reader spots that pair.
//!
//! So a reader is asked. The filing goes through first and a model is shown the
//! new subject against every open one; what comes back is printed under the
//! task, for the session to act on or ignore.
//!
//! ## Advisory, and it has to be
//!
//! ⚠ **The model half never refuses a filing and must not learn how.** Measured against
//! the live list on 2026-08-13, an all-pairs sweep over the 134 open titles
//! returned nine confident groups of which **one** was a real duplicate — the
//! rest were same-area, different-problem, which is the failure this exists to
//! avoid making worse. At that precision a gate would refuse correct filings
//! more often than it caught wrong ones, and a session that cannot file is a
//! session that stops writing things down.
//!
//! Asked the narrower question — *one* new subject against the list, which is
//! what actually happens at filing time — five replayed cases came back 5/5
//! correct: it found the two real overlaps (#760→#255, #777→#726) and stayed
//! quiet on the three that only looked related. That is the question this module
//! asks, and the reason it is worth asking at all.
//!
//! ## One exception, and it is not a guess
//!
//! [`same_subject`] does refuse, before anything is filed. The rule above is
//! about a *model reading titles* and the error rate that comes with it; string
//! equality has no error rate. See its own note for the filing that made the
//! distinction worth drawing.
//!
//! ## Open tasks only
//!
//! ⚠ **Closed tasks were tried and cost accuracy.** Re-running those same five
//! with the 662 done and 24 dropped rows included dropped it to 3/5 — #812
//! gained two false matches and #807 one — and took the slowest call from 17 to
//! 56 seconds. The argument for including them is real (re-filing something
//! *decided against* is the expensive mistake) and it lost to the measurement;
//! none of the five was a duplicate of a closed task, so what is measured here
//! is the cost of the 686 extra rows rather than the benefit. Worth revisiting
//! with a case that has one.
//!
//! ## Marked as inference, and cheap to dismiss
//!
//! What is printed is a guess, and says so. It carries ids rather than
//! conclusions so that checking one is `task show 255` — the same shape
//! `console/src/gist.rs` settled on in memview, and for the same reason: a
//! confidently wrong sentence about work somebody has not opened is worse than
//! no sentence.

/// The open task whose subject is already the one being filed, if there is one.
///
/// ⚠ **This is the half that is allowed to refuse, because it is not a guess.**
/// The module rule above — never block a filing — was measured on a model
/// reading titles, at a precision where a gate would have refused correct
/// filings more often than it caught wrong ones. Nothing in that measurement
/// reaches string equality, and equality is what the observed duplicate
/// actually was: #859 and #860 carried byte-identical subjects 46 seconds
/// apart, and only a model was looking for it.
///
/// Case and surrounding space are ignored. Neither distinguishes two pieces of
/// work — a subject that differs only in capitalisation is one sentence typed
/// twice — and both are exactly what a second attempt at one filing varies by.
pub fn same_subject(subject: &str, corpus: &[(u64, String)]) -> Option<u64> {
    let want = subject.trim();
    corpus
        .iter()
        .find(|(_, open)| open.trim().eq_ignore_ascii_case(want))
        .map(|(id, _)| *id)
}

/// What a refused filing says.
///
/// ⚠ **It names the three ways out, because two of them are the point.** A
/// collision is usually not "file it somewhere else" — it is an *update* to a
/// task that already exists, which is `task edit`. The override is last and
/// spelled in full so that nobody has to go and find it in `--help` while
/// holding a body on stdin.
pub fn collision(already: u64) -> String {
    format!(
        "#{already} is already open with this exact subject. Nothing was filed. \
         `task show {already}` to read it, `task edit {already}` if this is an update to it, \
         or re-run with --no-duplicate-check if they really are two tasks."
    )
}

/// A task the model thinks the new one might already be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// The task it named.
    pub id: u64,
    /// Its one clause about what the two share. Printed as given.
    pub why: String,
}

/// At most this many, however many come back.
///
/// The prompt asks for three and a model occasionally supplies five. A fourth
/// line is not a fourth candidate worth opening — by then the answer is a list
/// of everything in that area, which is the failure mode measured above.
const MOST: usize = 3;

/// What the model is asked.
///
/// `corpus` is `(id, subject)` for every open task, and **must not contain the
/// task just filed** — it is on the list by the time this runs, and a filing
/// that matches itself is the one answer guaranteed to be useless.
///
/// ⚠ **The negative half of the instruction is doing most of the work.** Asked
/// plainly whether two tasks are "similar", a model answers about subject
/// matter and returns every task in the same repository. Naming what is *not* a
/// duplicate — same area, same file, same technology — is what moved the
/// replayed cases from noise to 5/5.
pub fn prompt(subject: &str, corpus: &[(u64, String)]) -> String {
    let lines: String = corpus
        .iter()
        .map(|(id, subject)| format!("{id} | {subject}\n"))
        .collect();
    format!(
        "A session is about to file this task into a shared tracker:\n\n  {subject}\n\n\
         Below is every task already open, one per line, as `id | title`.\n\n\
         {lines}\n\
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

/// The matches in what came back, and nothing else.
///
/// ⚠ **Anything unparseable is dropped rather than shown.** This text is printed
/// under a task a session has just filed, and a model that decided to explain
/// itself would otherwise put a paragraph in every conversation's context. A
/// line that does not begin with an id is not an answer to the question asked.
///
/// `filed` is the id of the task that prompted this. It is excluded even though
/// [`prompt`] already leaves it out of the corpus, because a model that echoes
/// the number it was asked about would otherwise report a task as its own
/// duplicate — which reads as a service defect rather than as a bad guess.
pub fn parse(said: &str, filed: u64) -> Vec<Match> {
    let mut found: Vec<Match> = Vec::new();
    for line in said.lines() {
        let line = line.trim();
        // The whole answer in the ordinary case, and worth stopping on: a model
        // that says NONE and then explains why has said the useful part first.
        if line.eq_ignore_ascii_case("NONE") {
            break;
        }
        let Some(one) = one(line) else { continue };
        if one.id == filed || found.iter().any(|seen| seen.id == one.id) {
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
/// ⚠ **`1. #255: …` read as task #1**, which is how this function came to
/// exist: a model that numbers its answers would otherwise report tasks 1, 2
/// and 3 on every filing, and those are real tasks. Stripping any leading
/// `<digits>.` unconditionally would be worse — `255. the same cron` is a
/// plausible way to name task 255 — so the marker only goes when a `#` id
/// follows it, which is the case that is not ambiguous.
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

/// One line, if it is one.
///
/// Tolerant of the wrappers a model adds despite being asked not to: a leading
/// bullet or number, `#` on the id or not, and `--`, `—` or `:` between the id
/// and the reason. Not tolerant of a missing id, which is what separates an
/// answer from a sentence about the answer.
fn one(line: &str) -> Option<Match> {
    // Bullets and emphasis first, so `- **#255** -- ...` reaches the same path
    // as `#255 -- ...`. Backticks and asterisks go for the reason gist.rs
    // records: this is printed as plain text, so they arrive as punctuation.
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
    // An id with nothing after it is a number, not a finding. The prompt asks
    // for the clause because "#255" alone makes a reader open a task to learn
    // whether it was worth opening.
    (!why.is_empty()).then_some(Match { id, why })
}

/// How the matches are printed under a freshly filed task.
///
/// ⚠ **It says who is talking.** Every other line this CLI prints is read off
/// the service; this one is a guess by a small model from titles alone, and the
/// difference has to survive being pasted into a conversation without its
/// context.
///
/// ⚠ **It names the task it is about at both ends, and says how to undo it.**
/// The row `task add` writes to stdout carries the new id; this goes to stderr,
/// so a caller that pipes `2>&1 | tail -2` — the ordinary way a session keeps a
/// long filing quiet — keeps this and loses the id. Measured 2026-08-14: a
/// session filed #859, saw only a warning naming an unrelated #853, read it as
/// a refusal, and filed the same task again, because nothing left on screen
/// said a task had been created. Any tail of this is still actionable.
pub fn report(filed: u64, found: &[Match]) -> String {
    let mut out = format!(
        "\n#{filed} is filed. Something already open may cover it — a guess from titles:\n"
    );
    for one in found {
        out.push_str(&format!("  #{:<4} {}\n", one.id, one.why));
    }
    // Last, so that it is what survives `| tail -1`. The filing has already
    // landed, so the useful thing to hand back is not a decision but the one
    // command that reverses it.
    out.push_str(&format!("if one of them does: task drop {filed}\n"));
    out
}
