//! Turning what somebody typed after `move` into a conversation.
//!
//! ⚠ **Every place this tool prints a holder, it prints the NAME** — `(coach)`,
//! `(observe)` — and until 2026-08-10 the only thing it accepted was a
//! 36-character id, so its own output was not valid input to it. Assigning five
//! tasks meant first running `task sessions | grep` to translate three names
//! into uuids and pasting them back; the round trip existed only because the two
//! directions disagreed.
//!
//! ⚠ **What is NOT resolved is refused, and that is the important half.** Not
//! because the write would land — `fk_tasks_session` in `0001_init.sql` refuses
//! an assignee that has no `sessions` row, which was checked rather than assumed
//! — but because of *how* it is refused: the constraint surfaces as
//! `AppError::Other`, which is a 500 logged as "internal error" and reaching the
//! caller as `moving a task`. A typo deserves "no session called `helth`, did
//! you mean health", and that answer is free here, where the list of names has
//! just been fetched to decide the question anyway.
//!
//! This lives in the library rather than in the CLI for the reason
//! [`selection`](super::selection) does: it is a decision rather than plumbing,
//! and `tests/holder.rs` exercises it as public API instead of through a binary.

/// What a typed holder turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    /// One conversation, by id. Either it was typed as one, or its name
    /// resolved to exactly one.
    Session(String),
    /// The name belongs to more than one conversation.
    ///
    /// ⚠ **Not hypothetical: names are reused.** Two distinct sessions have both
    /// been called `memview`, and one of them is `dev-lint` now. Guessing here
    /// would hand work to whichever conversation happened to hold the name
    /// first, so both ids are returned and the caller is asked to choose.
    Ambiguous(Vec<String>),
    /// Nothing answers to it. Carries the names that do exist, because the
    /// reader's next question is always "what should I have typed".
    Unknown(Vec<String>),
}

///
/// **An exact id wins over a name.** They cannot collide in practice — one is a
/// uuid — but the id is the identity and the name is an attribute of it, so if
/// they ever did, the identity is the answer.
pub fn resolve<'a>(
    known: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
    typed: &str,
) -> Holder {
    let known: Vec<(&str, Option<&str>)> = known.into_iter().collect();
    if known.iter().any(|(id, _)| *id == typed) {
        return Holder::Session(typed.to_string());
    }
    let hit: Vec<&str> = known
        .iter()
        .filter(|(_, name)| *name == Some(typed))
        .map(|(id, _)| *id)
        .collect();
    match hit.as_slice() {
        [id] => Holder::Session((*id).to_string()),
        [] => {
            let mut names: Vec<String> = known
                .iter()
                .filter_map(|(_, name)| name.map(str::to_string))
                .collect();
            names.sort();
            names.dedup();
            Holder::Unknown(names)
        }
        many => Holder::Ambiguous(many.iter().map(|id| (*id).to_string()).collect()),
    }
}
