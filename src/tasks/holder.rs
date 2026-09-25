//! Turning what somebody typed after `move` into a conversation.
//!
//! ⚠ **Every place this tool prints a holder it prints the NAME**, so a name
//! has to be accepted as input — otherwise the tool's own output is not valid
//! input to it, and every handover starts with a `task sessions | grep` to
//! translate.
//!
//! ⚠ **What is NOT resolved is refused here.** `fk_tasks_session` would refuse
//! the write anyway, but as a 500 "internal error"; a typo deserves "no session
//! called that, did you mean …", and the names have just been fetched.
//!
//! In the library rather than the CLI: a decision, testable as public API
//! instead of through a binary.

/// What a typed holder turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    /// One conversation, by id. Either it was typed as one, or its name
    /// resolved to exactly one.
    Session(String),
    /// The name belongs to more than one conversation.
    ///
    /// ⚠ **Names are reused, and sessions get renamed.** Guessing would hand
    /// work to whichever held the name first, so every id is returned.
    Ambiguous(Vec<String>),
    /// Nothing answers to it. Carries the names that do exist, because the
    /// reader's next question is always "what should I have typed".
    Unknown(Vec<String>),
}

/// Resolve `typed` against the known `(id, name)` pairs.
///
/// **An exact id wins over a name**: the id is the identity, should a name ever
/// look like one.
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

/// Whether a filing's holder and its `--spare` reason fit each other.
///
/// ⚠ **Issued twice, so decided once.** The CLI refuses BEFORE the duplicate
/// check, so a filing that cannot land spends no model call; the service
/// refuses too, for the web form and any API caller. A CLI stricter than the
/// service would block filings the service accepts, so the verdict is shared
/// and only the WORDS differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PileVerdict {
    /// Held with no reason, or the pile with one.
    Fits,
    /// `--to nobody` with nothing saying why it is nobody's.
    PileNeedsReason,
    /// A reason for the pile on a task that has a holder. Refused rather than
    /// ignored: it would be stored against a task it does not describe.
    ReasonWithoutPile,
}

/// Decide it. `has_reason` must already have had blank reasons discarded — an
/// empty `--spare ""` is not a reason — or the two callers will disagree.
pub fn pile_verdict(to_nobody: bool, has_reason: bool) -> PileVerdict {
    match (to_nobody, has_reason) {
        (true, false) => PileVerdict::PileNeedsReason,
        (false, true) => PileVerdict::ReasonWithoutPile,
        _ => PileVerdict::Fits,
    }
}

impl PileVerdict {
    /// For somebody who typed a command: name the flag they typed, and say
    /// plainly that nothing was written.
    pub fn said_to_cli(self) -> &'static str {
        match self {
            PileVerdict::Fits => "",
            PileVerdict::PileNeedsReason => {
                "filing to the pile needs a reason: `--spare \"why this is nobody's\"`. \
                 Most work belongs to whoever files it, which is what `--to me` does by \
                 default. Nothing was filed."
            }
            PileVerdict::ReasonWithoutPile => {
                "`--spare` says why a task is nobody's, so it only fits `--to nobody`. \
                 Nothing was filed."
            }
        }
    }

    /// For the service, which answers the web form and any API caller as well —
    /// so it must not name a flag that only the CLI has.
    pub fn said_to_api(self) -> &'static str {
        match self {
            PileVerdict::Fits => "",
            PileVerdict::PileNeedsReason => {
                "filing to the pile needs a reason: say why this is nobody's, or file it \
                 to a holder"
            }
            PileVerdict::ReasonWithoutPile => {
                "a reason for the pile does not fit a task that has a holder"
            }
        }
    }
}
