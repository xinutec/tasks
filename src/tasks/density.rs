//! Whether a body that has grown without being read is still worth reading.
//!
//! A task body is written by whoever is holding it, in the order things
//! happened, and read by whoever picks it up next. Those are rarely the same
//! conversation and never the same moment, so the text drifts from being an
//! account of the work into being a transcript of somebody talking to
//! themselves — and nothing in the tracker noticed, because every individual
//! edit was reasonable.
//!
//! ## Measured, 2026-08-23, over nine days
//!
//! `task_revision` holds the body as it stood before each edit, so every edit
//! classifies exactly: the new body starts with the old (an append), ends with
//! it (a prepend), or neither (a rewrite). Of 667 body-changing edits, 329 were
//! appends adding 540,840 characters and 130 were prepends adding 250,560,
//! against 183 rewrites removing 26,593. The corpus grows by accretion and
//! shrinks only when somebody rewrites. #982 ran 42 consecutive growing edits,
//! 2,795 → 100,382 characters, without once being consolidated.
//!
//! ⚠ **Removing `--prepend` was considered and refuted by the same data.** In
//! the 1.8 days of history before those flags existed, 87 of 225 body-changing
//! edits were exact appends and prepends typed by hand as `--body "<new>\n\n<the
//! whole old body>"`; #445 grew 6,015 → 32,506 characters in eleven of them, an
//! hour apart. A session that pastes the body back has read it into context and
//! appends anyway. Taking the flags away returns to that regime and makes
//! `--body` the only way to add a line, which is the write that put `True` over
//! 3,109 characters of #900.
//!
//! ## A sampler, then a judge
//!
//! [`worth_asking`] is arithmetic and deterministic: characters grown since the
//! last edit that made the body smaller. It decides WHEN the question is asked,
//! and nothing else. The question itself goes to a model, because the defect is
//! semantic — #749 was 76% superseded, and no threshold can see that.
//!
//! ⚠ **Never a refusal.** `duplicates.rs` refuses, and the difference is what
//! is being judged: whether a title is already on a list is a fact about that
//! list, and whether prose is dense is taste. A gate on taste that fires on one
//! edit and not the identical next one teaches sessions to edit again until it
//! goes quiet. The write has already landed by the time this runs, which is the
//! same reason: advice after the fact is advice, and a refusal after the fact is
//! nothing at all.
//!
//! ⚠ **Never "as few words as possible".** Told to compress, a model drops the
//! numbers and keeps the prose, because prose reads like the argument — and a
//! body is believed for its measurements. [`RUBRIC`] therefore asks for density
//! and lets length be its consequence, and its second rule is deliberately
//! asymmetric: it cuts restatement and never evidence.

/// Characters of unconsolidated growth before the question is worth asking.
///
/// **Three thousand, from the distribution rather than from taste.** The median
/// growing edit over the nine days measured was 1,476 characters, so this is
/// roughly two ordinary additions; across the whole fleet it would have fired on
/// 56 tasks in nine days, about six a day. That rate is the point — the check
/// costs the session that ran the command 8-24 seconds of its own clock and its
/// own allowance, and asking on every one of the 74 daily body edits would buy
/// nothing for the ~90% that are one paragraph landing on a body somebody wrote
/// this morning.
pub const SAMPLER: usize = 3_000;

/// Whether this much unconsolidated growth is worth a model's opinion.
pub fn worth_asking(accreted: usize) -> bool {
    accreted > SAMPLER
}

/// The standard a body is held to, stated once and used twice.
///
/// ⚠ **The judge applies it and `task edit --help` prints it, from this same
/// string.** A standard stated only to the judge arrives after the writing;
/// stated in the help it arrives before, which is the only place it can prevent
/// anything. Two copies would drift, and the copy that drifts is the one nobody
/// is grading against.
pub const RUBRIC: &str = "\
1. Every paragraph earns its place one of three ways: it tells the holder what \
to do, it is the evidence for that, or it records a refutation that stops \
somebody redoing dead work. Nothing else stays. Completeness here is relative \
to ONE reader — the holder, about to do the work — and ONE question: what do I \
do, and why is that right?
2. No claim without its measurement, and no sentence that only restates one.
3. Deletion beats compression. Rewording caps out around 20%; #749 went 16,405 \
to 3,928 characters because 76% of it was superseded, not because it was wordy. \
So look for SUPERSESSION, not verbosity.";

/// The single word that means there is nothing to say.
const FINE: &str = "DENSE";

/// What is put to the model, and what it may answer.
pub fn prompt(id: u64, accreted: usize, body: &str) -> String {
    format!(
        "The body of task #{id} in a shared tracker has grown {accreted} characters since \
         anybody last rewrote it. Several conversations add to these, none of them reading \
         the whole thing first.\n\n\
         Judge whether it still reads as one document, against this standard:\n\n\
         {RUBRIC}\n\n\
         Answer with ONE FINDING PER LINE and at most four lines, addressed to the session \
         that holds it.\n\
         If it still holds together, answer with exactly: {FINE}\n\
         Otherwise say SPECIFICALLY what is wrong — where the conclusion sits, which section \
         is superseded by which, which paragraph carries no claim. Quote the headings you \
         mean. No preamble, no praise, no summary of what the task is about: the holder \
         wrote it and knows.\n\n\
         The body follows, between the markers.\n\n\
         -----BEGIN BODY-----\n{body}\n-----END BODY-----"
    )
}

/// What to print, if anything.
///
/// ⚠ **The four-line bound is a bound on the model's manners, not on the
/// output.** Asked for four lines on 2026-08-23 it returned one, 600 characters
/// wide, containing four findings — correct ones, in a wall. `ONE FINDING PER
/// LINE` in [`prompt`] is the fix, and it is worth knowing that it is a request
/// rather than a limit: truncating mid-sentence would cut the specific half of a
/// finding, which is the half worth having.
///
/// ⚠ **An unreadable answer is silence, not a warning.** This runs after a write
/// that has already landed, so there is nothing for a failure to protect; a line
/// that cannot say what is wrong with the body would be a session's attention
/// spent on the checker rather than on the task.
pub fn advice(said: &str, id: u64) -> Option<String> {
    let said = said.trim();
    if said.is_empty() || said.eq_ignore_ascii_case(FINE) {
        return None;
    }
    // A model that answers DENSE and then explains itself has said the useful
    // part first, exactly as `duplicates::parse` treats NONE.
    let first = said.lines().next().map(str::trim).unwrap_or_default();
    if first.eq_ignore_ascii_case(FINE) {
        return None;
    }
    let mut out = String::from("a model read this body, and it is a guess:\n");
    // ⚠ **Blank lines are not findings, and taking four LINES gave two.** Asked
    // for one finding per line on 2026-08-23, the model separated them with
    // blank ones and half the answer fell off the end of the bound meant to
    // keep it short. The bound is on what it found.
    for line in said
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(4)
    {
        out.push_str(&format!("  {line}\n"));
    }
    out.push_str(&format!(
        "  `task edit {id} --body -` is how it gets rewritten."
    ));
    Some(out)
}
