//! Turning a typed holder into a conversation. The tool prints holders by name,
//! so it takes names as input. An unknown one is refused here with the names
//! that exist, where the foreign key would only answer 500.

/// What a typed holder turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    /// One conversation, by id, typed or resolved from its name.
    Session(String),
    /// More than one conversation answers to it (names are reused); guessing
    /// would misroute work.
    Ambiguous(Vec<String>),
    /// Nothing answers to it; carries the names that do exist.
    Unknown(Vec<String>),
}

/// Resolve `typed` against `(id, name)` pairs.
///
/// ⚠ A word that is one conversation's id and another's name is ambiguous, not
/// the id's: ids are not all UUIDs (`TASKS_SESSION=claude-sync`), and picking
/// the id silently misroutes everything sent to the name.
pub fn resolve<'a>(
    known: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
    typed: &str,
) -> Holder {
    let known: Vec<(&str, Option<&str>)> = known.into_iter().collect();
    let mut hit: Vec<&str> = known
        .iter()
        .filter(|(id, name)| *id == typed || *name == Some(typed))
        .map(|(id, _)| *id)
        .collect();
    hit.dedup();
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

/// Whether a filing's holder and its `--spare` reason fit. Shared by the CLI
/// (which refuses before spending a model call) and the service, so they
/// cannot disagree; only the wording differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PileVerdict {
    /// Held with no reason, or the pile with one.
    Fits,
    /// `--to nobody` with nothing saying why it is nobody's.
    PileNeedsReason,
    /// A pile reason on a task with a holder: it would describe nothing.
    ReasonWithoutPile,
}

/// `has_reason` must already exclude blanks (`--spare ""`), or the callers
/// disagree.
pub fn pile_verdict(to_nobody: bool, has_reason: bool) -> PileVerdict {
    match (to_nobody, has_reason) {
        (true, false) => PileVerdict::PileNeedsReason,
        (false, true) => PileVerdict::ReasonWithoutPile,
        _ => PileVerdict::Fits,
    }
}

impl PileVerdict {
    /// Names the flag, and says nothing was filed.
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

    /// For the service, which also answers the web form: no CLI flags.
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
