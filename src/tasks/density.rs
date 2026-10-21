//! Whether a body that has grown without anyone rewriting it should be.
//!
//! Bodies grow by append and prepend, written by one conversation and read by
//! the next, until earlier layers contradict later ones. [`worth_asking`] is
//! arithmetic and only decides when to ask; a model judges, because
//! supersession is semantic.
//!
//! ⚠ It never refuses: the edit has landed, and a gate on taste teaches
//! sessions to edit until it goes quiet. It speaks only for a contradiction or
//! a rewrite that would remove a third, or it would speak on every body.
//! [`RUBRIC`] asks for density, never brevity: told to compress, a model drops
//! the numbers and keeps the prose.

/// Growth since the body last shrank before a model is asked: about two
/// ordinary additions, from the distribution of edits.
pub const SAMPLER: usize = 3_000;

/// Whether this much unconsolidated growth is worth a model's opinion.
pub fn worth_asking(accreted: usize) -> bool {
    accreted > SAMPLER
}

/// The standard a body is held to. `task edit --help` prints it too, so a
/// writer sees it before writing, not only after.
pub const RUBRIC: &str = "\
1. Every paragraph earns its place one of three ways: it tells the holder what \
to do, it is the evidence for that, or it records a refutation that stops \
somebody redoing dead work. Nothing else stays. Completeness here is relative \
to ONE reader — the holder, about to do the work — and ONE question: what do I \
do, and why is that right?
2. No claim without its measurement, and no sentence that only restates one.
3. Deletion beats compression. Rewording saves a little; a body that is mostly \
superseded by its own later text can lose most of its length. So look for \
SUPERSESSION, not verbosity.";

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
         Speak ONLY for one of two things. First, it contradicts itself: an earlier \
         part states as current what a later part replaced, and both still stand. Name \
         both, however short. Second, rewriting it to that standard would remove at \
         least a third of it. A section that could be tighter, reordered or better \
         placed is NOT worth a rewrite, and neither is history on a closed task. Most \
         bodies pass.\n\n\
         If it passes, answer with exactly: {FINE}\n\
         Otherwise answer with ONE FINDING PER LINE and at most four lines, addressed to \
         the session that holds it.\n\
         Otherwise say SPECIFICALLY what is wrong — where the conclusion sits, which section \
         is superseded by which, which paragraph carries no claim. Quote the headings you \
         mean. No preamble, no praise, no summary of what the task is about: the holder \
         wrote it and knows.\n\n\
         The body follows, between the markers.\n\n\
         -----BEGIN BODY-----\n{body}\n-----END BODY-----"
    )
}

/// What to print, if anything. At most four findings, never cut mid-line; an
/// unreadable answer prints nothing.
pub fn advice(said: &str, id: u64) -> Option<String> {
    let said = said.trim();
    if said.is_empty() || said.eq_ignore_ascii_case(FINE) {
        return None;
    }
    // DENSE followed by an explanation is still DENSE, as `duplicates::parse`
    // treats NONE.
    let first = said.lines().next().map(str::trim).unwrap_or_default();
    if first.eq_ignore_ascii_case(FINE) {
        return None;
    }
    let mut out = String::from("a model read this body, and it is a guess:\n");
    // Blank lines don't count against the four.
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
