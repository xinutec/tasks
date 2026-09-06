//! Falsifying what a review said about a task body.
//!
//! A model asked to review prose returns findings that all read alike: the true
//! ones and the invented ones arrive in the same confident shape. On 2026-09-06,
//! fifty agents over all 712 closed bodies returned 235 findings and **81 did
//! not survive the checks below** — 27 quoting text that appears in no task they
//! were given, 28 claiming a supersession that runs backwards.
//!
//! ⚠ **The requirement that did the work is quoting the LATER text.** Saying
//! "this paragraph is stale" costs nothing and cannot be checked. Saying "this
//! paragraph is stale AND here is the verbatim text further down that
//! supersedes it" is falsifiable by string position, and it rejected **51 of 56**
//! stale-layer claims. Nothing about the model changed; the shape of the answer
//! did.
//!
//! ⚠ **This module holds the decision and none of the IO**, so its tests are
//! the past mistakes rather than a fixture: no database, no network, no task
//! store. `tests/findings.rs` pins both ways the first instrument was wrong.
//!
//! ⚠ **A finding is checkable only against the text it was made about, and that
//! is why the body is passed in rather than fetched.** Re-running the 2026-09-06
//! findings after their 130 subject rewrites refused 225 of 235 — correctly, and
//! uselessly: the quoted text no longer existed because it had been repaired.
//! The same run before the edits refused 81. Neither number is wrong; they are
//! answers about different corpora. Check a review while its corpus still
//! stands, or keep the revision it was made against.

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
    /// Whether this claim is ABOUT one span superseding another, which is the
    /// only class that can be checked by position.
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
    /// For a repetition claim: the SECOND occurrence.
    ///
    /// ⚠ **Required, and that is instrument failure 2.** Counting occurrences
    /// of `quote_problem` alone called all 14 repetition findings false, because
    /// a model quotes one instance together with a lead-in that appears once.
    /// #684 really does carry the same `scp`/`ssh` pair twice; the check said it
    /// did not.
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

/// The outcome. Surviving this is not being right — it is only being checkable
/// and not caught. Every survivor still wants reading before anybody edits on it.
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

/// Try to refute a finding from the task's own text.
///
/// ⚠ **The subject is searched too, and that is instrument failure 1.** The
/// first version looked in the body alone and rejected 130 correct
/// `subject-stale` findings, because `task show --body` does not include the
/// subject. A quote is checked against subject AND body, always — cheaper than
/// deciding per kind which one it should have been.
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
        // Two DISTINCT positions. Quoting the same span twice must not pass, so
        // the second is searched after the end of the first.
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
