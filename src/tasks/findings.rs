//! Falsifying what a model's review said about a task body.
//!
//! True and invented findings read alike. Requiring a supersession claim to
//! quote the later text that supersedes is what makes it checkable, by string
//! position, and that rejects most invented ones.
//!
//! The body is passed in, not fetched: a finding can only be checked against
//! the text it was made about, and once applied that text is gone.

/// What a finding claims about a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Text left standing that later text supersedes.
    StaleLayer,
    /// Two parts of the body disagree.
    Contradiction,
    /// The subject says something the body has moved past.
    SubjectStale,
    /// The same fact stated more than once with no added measurement.
    Repetition,
    /// Prose that could go with no loss.
    Verbose,
    /// Not a defect — something valuable a careless tidy-up would delete.
    InfoAtRisk,
}

impl Kind {
    /// The claims checkable by position: one span superseding another.
    fn claims_supersession(self) -> bool {
        matches!(self, Kind::StaleLayer | Kind::Contradiction)
    }
}

/// One claim, with the evidence it is required to carry.
#[derive(Debug, Clone)]
pub struct Finding {
    pub kind: Kind,
    /// The text said to be wrong, stale or repeated. Verbatim.
    pub quote_problem: String,
    /// For a supersession claim: the later text that settles it. Verbatim.
    pub quote_resolving: Option<String>,
    /// For a repetition claim: the second occurrence. Required, since a quote
    /// usually carries a lead-in that appears only once.
    pub quote_second: Option<String>,
}

/// Why a finding was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The quoted text is in neither the subject nor the body.
    ProblemNotVerbatim,
    /// A supersession claim with nothing quoted to supersede with.
    NoResolvingQuote,
    /// The resolving text is not in the task either.
    ResolvingNotVerbatim,
    /// The resolving text comes BEFORE the text it supposedly supersedes.
    ResolvingComesFirst,
    /// A repetition claim naming fewer than two distinct occurrences.
    RepetitionNeedsBoth,
}

/// Surviving is not being right, only not being caught: read a survivor
/// before editing on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Supported,
    Rejected(Vec<Reason>),
}

impl Verdict {
    pub fn supported(&self) -> bool {
        matches!(self, Verdict::Supported)
    }
    pub fn rejected_for(&self, why: Reason) -> bool {
        matches!(self, Verdict::Rejected(rs) if rs.contains(&why))
    }
}

/// Try to refute a finding from the task's own text, subject included (or
/// every `subject-stale` finding would fail).
pub fn falsify(finding: &Finding, subject: &str, body: &str) -> Verdict {
    let hay = format!("{subject}\n{body}");
    let mut refused = Vec::new();

    let problem = finding.quote_problem.trim();
    let at = if problem.is_empty() {
        refused.push(Reason::ProblemNotVerbatim);
        None
    } else {
        match hay.find(problem) {
            Some(i) => Some(i),
            None => {
                refused.push(Reason::ProblemNotVerbatim);
                None
            }
        }
    };

    if finding.kind.claims_supersession() {
        match finding.quote_resolving.as_deref().map(str::trim) {
            None | Some("") => refused.push(Reason::NoResolvingQuote),
            Some(later) => match hay.find(later) {
                None => refused.push(Reason::ResolvingNotVerbatim),
                Some(j) => {
                    if let Some(i) = at
                        && j < i
                    {
                        refused.push(Reason::ResolvingComesFirst);
                    }
                }
            },
        }
    }

    if finding.kind == Kind::Repetition {
        let second = finding.quote_second.as_deref().map(str::trim).unwrap_or("");
        // Searched after the first, so the same span quoted twice fails.
        let ok = match (at, second.is_empty()) {
            (Some(i), false) => hay[i + problem.len()..].contains(second),
            _ => false,
        };
        if !ok {
            refused.push(Reason::RepetitionNeedsBoth);
        }
    }

    if refused.is_empty() {
        Verdict::Supported
    } else {
        Verdict::Rejected(refused)
    }
}
