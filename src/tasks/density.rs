//! Whether a body that has grown without being read is still worth reading.
//!
//! A task body is written by whoever is holding it, in the order things
//! happened, and read by whoever picks it up next. Those are rarely the same
//! conversation and never the same moment, so the text drifts from being an
//! account of the work into being a transcript of somebody talking to
//! themselves — and nothing in the tracker noticed, because every individual
//! edit was reasonable.
//!
//! ## Accretion is the shape, and it is measurable
//!
//! `task_revision` holds the body as it stood before each edit, so every edit
//! classifies exactly: the new body starts with the old (an append), ends with
//! it (a prepend), or neither (a rewrite). Appends and prepends dominate. The
//! corpus grows by accretion and shrinks only when somebody rewrites, and a
//! single task can run dozens of growing edits without once being consolidated.
//!
//! ⚠ **Removing `--prepend` was considered and refuted by the same data.**
//! Before those flags existed, sessions typed the same edits by hand as
//! `--body "<new>\n\n<the whole old body>"` — a session that pastes the body
//! back has read it into context and appends anyway. Taking the flags away
//! returns to that regime and makes `--body` the only way to add a line, which
//! is the write that silently replaces a whole body with one word.
//!
//! ## A sampler, then a judge
//!
//! [`worth_asking`] is arithmetic and deterministic: characters grown since the
//! last edit that made the body smaller. It decides WHEN the question is asked,
//! and nothing else. The question itself goes to a model, because the defect is
//! semantic: a body can be mostly superseded by its own later layers, and no
//! threshold can see that.
//!
//! ⚠ **Never a refusal.** `duplicates.rs` refuses because whether a title is
//! already on a list is a fact about that list; whether prose is dense is
//! taste. A gate on taste that fires on one edit and not the identical next
//! teaches sessions to edit until it goes quiet. And the write has landed by
//! the time this runs.
//!
//! ⚠ **The judge has a bar, or it always speaks.** Asked what is wrong with a
//! body against [`RUBRIC`], a model finds something in nearly every one,
//! bodies just rewritten included — and a check that speaks on everything is a
//! banner. So [`prompt`] lets it speak only for a self-contradiction, however
//! short, or a rewrite that would remove a third. Calibrated against bodies
//! whose last edit was a consolidating rewrite, which should pass: a change to
//! the prompt is judged by how often it speaks on those as well as on grown
//! ones.
//!
//! ⚠ **Never "as few words as possible".** Told to compress, a model drops the
//! numbers and keeps the prose, because prose reads like the argument — and a
//! body is believed for its measurements. [`RUBRIC`] therefore asks for density
//! and lets length be its consequence, and its second rule is deliberately
//! asymmetric: it cuts restatement and never evidence.

/// Characters of unconsolidated growth before the question is worth asking.
///
/// **From the distribution, not taste**: roughly two ordinary additions. The
/// check spends the caller's clock and allowance, and most edits are one
/// paragraph on a body that was recently whole.
pub const SAMPLER: usize = 3_000;

/// Whether this much unconsolidated growth is worth a model's opinion.
pub fn worth_asking(accreted: usize) -> bool {
    accreted > SAMPLER
}

/// The standard a body is held to, stated once and used twice.
///
/// ⚠ **The judge applies it and `task edit --help` prints it, from this same
/// string.** Stated only to the judge it arrives after the writing; in the help
/// it arrives before, where it can prevent something.
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

/// What to print, if anything.
///
/// ⚠ **The line bound is on findings, not characters.** Asked for four lines a
/// model returns one wide one holding four, so [`prompt`] asks for `ONE FINDING
/// PER LINE`; truncating mid-sentence would cut the specific half of a finding.
///
/// ⚠ **An unreadable answer is silence, not a warning**: the write has landed,
/// and a line about the checker spends attention that belongs to the task.
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
    // ⚠ **Blank lines are not findings.** A model asked for one finding per
    // line separates them with blank ones, and a bound on LINES then drops half
    // the answer. The bound is on what it found.
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
