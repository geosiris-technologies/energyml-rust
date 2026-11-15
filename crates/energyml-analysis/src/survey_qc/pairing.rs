//! Which trajectory goes with which survey: a pure rule, fed with the links each face finds.

use energyml_geometry::stations::same_object;

/// What an object is, for pairing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurveyObjectKind {
    /// A WITSML `Trajectory`.
    WitsmlTrajectory,
    /// A RESQML `WellboreTrajectoryRepresentation`.
    ResqmlTrajectory,
    /// A RESQML 2.0.1 `DeviationSurveyRepresentation`.
    DeviationSurvey,
}

/// What the caller's links say about one object (full uris only).
#[derive(Debug, Clone, PartialEq)]
pub struct TrajectoryRefs {
    /// The object's own uri.
    pub uri: String,
    /// What it is.
    pub kind: SurveyObjectKind,
    /// A RESQML trajectory's `WitsmlTrajectory`.
    pub witsml_trajectory: Option<String>,
    /// A RESQML 2.0.1 trajectory's `DeviationSurvey`.
    pub deviation_survey: Option<String>,
    /// The MD datum the object counts from.
    pub md_datum: Option<String>,
    /// First and last MD, metres.
    pub md_range: Option<[f64; 2]>,
    /// The `Activity` objects that name this object as a `DataObject`.
    pub activities: Vec<String>,
}

/// The evidence on which two objects were paired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairedBy {
    /// A RESQML trajectory names the WITSML one (or the other way round).
    WitsmlTrajectory,
    /// A trajectory names the deviation survey (or the other way round).
    DeviationSurvey,
    /// Both are data objects of one activity, and they are not of the same kind.
    Activity,
    /// The same MD datum and overlapping MD ranges: a proposal, never a choice.
    SameDatum,
}

/// The candidates found for one subject.
#[derive(Debug, Clone, PartialEq)]
pub struct Pairings {
    /// The evidence that answered, `None` when none did.
    pub by: Option<PairedBy>,
    /// The uris of the candidates, in the order given.
    pub candidates: Vec<String>,
    /// The one candidate when the evidence is stated by the file and names exactly one; never for `SameDatum`.
    pub chosen: Option<String>,
}

fn names(link: &Option<String>, uri: &str) -> bool {
    link.as_deref().is_some_and(|l| same_object(l, uri))
}

fn shares_activity(a: &TrajectoryRefs, b: &TrajectoryRefs) -> bool {
    a.activities
        .iter()
        .any(|x| b.activities.iter().any(|y| same_object(x, y)))
}

fn same_datum(a: &TrajectoryRefs, b: &TrajectoryRefs) -> bool {
    let datum = match (&a.md_datum, &b.md_datum) {
        (Some(x), Some(y)) => same_object(x, y),
        _ => false,
    };
    let overlap = match (a.md_range, b.md_range) {
        (Some(x), Some(y)) => x[0].max(y[0]) < x[1].min(y[1]),
        _ => false,
    };
    datum && overlap
}

/// Whether `candidate` answers `subject` by this evidence.
fn answers(by: PairedBy, subject: &TrajectoryRefs, candidate: &TrajectoryRefs) -> bool {
    match by {
        PairedBy::WitsmlTrajectory => {
            names(&subject.witsml_trajectory, &candidate.uri) || names(&candidate.witsml_trajectory, &subject.uri)
        }
        PairedBy::DeviationSurvey => {
            names(&subject.deviation_survey, &candidate.uri) || names(&candidate.deviation_survey, &subject.uri)
        }
        PairedBy::Activity => candidate.kind != subject.kind && shares_activity(subject, candidate),
        PairedBy::SameDatum => candidate.kind != subject.kind && same_datum(subject, candidate),
    }
}

/// The objects that go with `subject` among `candidates`, by four kinds of evidence taken in this order, the first
/// that finds at least one answering: the WITSML trajectory link, the deviation survey link, a shared activity (of
/// objects of two kinds: an activity naming ten RESQML trajectories pairs no two of them), the same MD datum
/// over overlapping MD ranges. The subject itself is never its own candidate.
///
/// `chosen` is set for the first three, when exactly one candidate answers: the file states it. The fourth is a
/// proposal and is never chosen; the user confirms it.
pub fn pairings(subject: &TrajectoryRefs, candidates: &[TrajectoryRefs]) -> Pairings {
    let others: Vec<&TrajectoryRefs> = candidates.iter().filter(|c| !same_object(&c.uri, &subject.uri)).collect();
    let rules = [
        PairedBy::WitsmlTrajectory,
        PairedBy::DeviationSurvey,
        PairedBy::Activity,
        PairedBy::SameDatum,
    ];
    for by in rules {
        let found: Vec<String> = others.iter().filter(|c| answers(by, subject, c)).map(|c| c.uri.clone()).collect();
        if found.is_empty() {
            continue;
        }
        let chosen = (by != PairedBy::SameDatum && found.len() == 1).then(|| found[0].clone());
        return Pairings {
            by: Some(by),
            candidates: found,
            chosen,
        };
    }
    Pairings {
        by: None,
        candidates: Vec::new(),
        chosen: None,
    }
}
