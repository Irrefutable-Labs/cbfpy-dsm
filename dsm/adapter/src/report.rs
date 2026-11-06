//! What the AI's side is told about its futures: verdicts, reasons and plain-language guidance,
//! with positions for display. The positions show how a candidate would move each band, so the
//! agent process can work out a measurement from them. The model's prompt never includes them.

use crate::enforcer::{Future, FutureStatus};
use crate::fork::Outcome;
use ethical_band_engine::{Evaluation, Policy, Rejection};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Movement {
    pub dim: String,
    pub from: i64,
    pub to: i64,
    pub allowed_outward: i64,
    pub stops_at: i64,
    pub restores_past: i64,
    pub must_return: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FutureReport {
    pub index: usize,
    pub action: String,
    pub outcome: Outcome,
    pub reasons: Vec<String>,
    /// Which way to go, in words. This is the pull back toward center that the AI hears.
    pub guidance: Vec<String>,
    pub movements: Vec<Movement>,
}

fn name(policy: &Policy, dim: usize) -> String {
    policy
        .bands
        .get(dim)
        .map_or("?".to_string(), |b| b.name.clone())
}

pub fn explain(r: &Rejection, policy: &Policy) -> String {
    match r {
        Rejection::Prohibited { tag } => format!("prohibited act ({tag})"),
        Rejection::WrongDimensionCount { expected, got } => {
            format!("wrong number of dimensions: {got} for {expected}")
        }
        Rejection::OutOfRange { dim } => format!("{}: out of numeric range", name(policy, *dim)),
        Rejection::ParentOutsideHardLimit { dim } => {
            format!("{}: parent is outside the hard limit", name(policy, *dim))
        }
        Rejection::HardLimit { dim } => {
            format!("{}: would cross the hard limit", name(policy, *dim))
        }
        Rejection::MaxStep { dim } => {
            format!("{}: larger than the maximum step", name(policy, *dim))
        }
        Rejection::OutwardExceeded {
            dim,
            outward,
            allowed,
        } => format!(
            "{}: outward movement exceeds the allowance ({outward} > {allowed})",
            name(policy, *dim)
        ),
        Rejection::RestoreRequired { dim, required } => format!(
            "{}: must move back toward the center by at least {required}",
            name(policy, *dim)
        ),
    }
}

pub fn guide(r: &Rejection, policy: &Policy) -> String {
    let n = |d: &usize| name(policy, *d).replace('_', " ");
    match r {
        Rejection::Prohibited { .. } => "That act is prohibited, whatever the circumstances.".to_string(),
        Rejection::OutwardExceeded { dim, .. } => format!(
            "{} cannot move further from its center now. Choose something that holds it steady or brings it back.",
            n(dim)
        ),
        Rejection::RestoreRequired { dim, .. } => format!(
            "{} has gone too far and must come back toward its center. Choose something that brings it back.",
            n(dim)
        ),
        Rejection::MaxStep { dim } => format!("That is too large a change in {} at once.", n(dim)),
        Rejection::HardLimit { dim } => format!("That would take {} past a hard limit.", n(dim)),
        _ => "That cannot be evaluated.".to_string(),
    }
}

pub fn movements(e: &Evaluation, policy: &Policy) -> Vec<Movement> {
    policy
        .bands
        .iter()
        .zip(&e.dims)
        .filter(|(_, d)| d.deviation != d.next_deviation || d.band.forced_progress > 0)
        .map(|(b, d)| {
            let (stops_at, restores_past) = if d.next_deviation >= 0 {
                (d.band.no_outward_pos, d.band.restore_pos)
            } else {
                (d.band.no_outward_neg, d.band.restore_neg)
            };
            Movement {
                dim: b.name.clone(),
                from: d.deviation,
                to: d.next_deviation,
                allowed_outward: d.allowed_outward,
                stops_at,
                restores_past,
                must_return: d.band.forced_progress,
            }
        })
        .collect()
}

pub fn report(futures: &[Future], policy: &Policy) -> Vec<FutureReport> {
    futures
        .iter()
        .map(|f| match &f.status {
            FutureStatus::Eligible(e) => FutureReport {
                index: f.index,
                action: f.proposal.action.clone(),
                outcome: Outcome::Eligible,
                reasons: vec![],
                guidance: vec![],
                movements: movements(e, policy),
            },
            FutureStatus::Rejected(e) => FutureReport {
                index: f.index,
                action: f.proposal.action.clone(),
                outcome: Outcome::Refused,
                reasons: e.rejections.iter().map(|r| explain(r, policy)).collect(),
                guidance: e.rejections.iter().map(|r| guide(r, policy)).collect(),
                movements: movements(e, policy),
            },
            FutureStatus::Malformed => FutureReport {
                index: f.index,
                action: f.proposal.action.clone(),
                outcome: Outcome::Malformed,
                reasons: vec![
                    "its text is missing, too long or unprintable for what it does".to_string(),
                ],
                guidance: vec![
                    "That action needs a short, plain text saying what it will say or write."
                        .to_string(),
                ],
                movements: vec![],
            },
            FutureStatus::Unmeasured => FutureReport {
                index: f.index,
                action: f.proposal.action.clone(),
                outcome: Outcome::Unmeasured,
                reasons: vec!["no signed measurement exists for this action".to_string()],
                guidance: vec![
                    "That is not a known action. Choose one of the known actions.".to_string(),
                ],
                movements: vec![],
            },
        })
        .collect()
}
