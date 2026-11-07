//! What the AI's side is told about its futures: verdicts, reasons and plain-language guidance,
//! with what each body act would reach. A refusal always comes with its reason, and the guidance
//! says what would be allowed instead: look first, do less, ask, or decline.

use crate::enforcer::{Future, FutureStatus};
use crate::fork::Outcome;
use ethical_world::check::{explain, Effect, Refusal, Severity};
use ethical_world::Cell;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FutureReport {
    pub index: usize,
    pub action: String,
    /// The body act, for a body act.
    pub body: Option<ethical_world::Act>,
    pub outcome: Outcome,
    pub reasons: Vec<String>,
    /// What to do instead, in words.
    pub guidance: Vec<String>,
    /// What a body act could physically affect, and the worst it could do there.
    pub reach: Vec<Cell>,
    pub severity: Option<Severity>,
    /// What it would do to known beings' quality of life, raises included.
    pub effects: Vec<Effect>,
    /// The purpose the candidate declared, if it declared one.
    pub purpose: Option<String>,
}

pub fn guide(r: &Refusal) -> String {
    match r {
        Refusal::Prohibited { .. } => {
            "That is never done, whatever you believe or have seen. Decline, and say why.".to_string()
        }
        Refusal::Blind { .. } => "Your senses are not working, so your body does nothing. You can still think and speak: say what is wrong, or look again once they are fixed.".to_string(),
        Refusal::TargetsBeing { .. } => {
            "Never use force on a protected being. Find another way, or decline.".to_string()
        }
        Refusal::Reaches { .. } => "Someone is in the way. Ask them to move, wait, or choose an act that does not reach them.".to_string(),
        Refusal::Unseen { .. } => "Look first: observe that space, from somewhere you can see into it, enough times and recently enough. Or do less, keeping to what you have already seen clear.".to_string(),
        Refusal::TooFast { .. } => "Go slower, or look further ahead first.".to_string(),
        Refusal::OverTopSpeed { .. } => "Go slower.".to_string(),
        Refusal::OffRoad { .. } => "Keep to road you have seen.".to_string(),
        Refusal::Lowers(_) => "That would leave someone worse off than they are now. Leave things as they are, or decline and say why.".to_string(),
        Refusal::Malformed { .. } => "That is not something you can do from where you are.".to_string(),
    }
}

fn base(f: &Future, outcome: Outcome) -> FutureReport {
    FutureReport {
        index: f.index,
        action: f.proposal.action.clone(),
        body: f.proposal.body.clone(),
        outcome,
        reasons: vec![],
        guidance: vec![],
        reach: vec![],
        severity: None,
        effects: vec![],
        purpose: f.proposal.declaration.as_ref().map(|d| d.purpose.clone()),
    }
}

pub fn report(futures: &[Future]) -> Vec<FutureReport> {
    futures
        .iter()
        .map(|f| match &f.status {
            FutureStatus::Eligible(e) | FutureStatus::Rejected(e) => {
                let eligible = matches!(f.status, FutureStatus::Eligible(_));
                FutureReport {
                    reasons: e.verdict.refusals.iter().map(explain).collect(),
                    guidance: e.verdict.refusals.iter().map(guide).collect(),
                    reach: e.verdict.reach.clone(),
                    severity: f.proposal.body.as_ref().map(|_| e.verdict.severity),
                    effects: e.verdict.effects.clone(),
                    ..base(
                        f,
                        if eligible {
                            Outcome::Eligible
                        } else {
                            Outcome::Refused
                        },
                    )
                }
            }
            FutureStatus::Deceptive(why) => FutureReport {
                reasons: why.clone(),
                guidance: why
                    .iter()
                    .map(|w| {
                        format!(
                            "Say only what is true, only as surely as it is known, and declare it: {w}."
                        )
                    })
                    .collect(),
                ..base(f, Outcome::Deceptive)
            },
            FutureStatus::Undisclosed(why) => FutureReport {
                reasons: why.clone(),
                guidance: why.iter().map(|w| format!("Declare it in full: {w}.")).collect(),
                ..base(f, Outcome::Undisclosed)
            },
            FutureStatus::Malformed => FutureReport {
                reasons: vec!["it is not a well-formed act, or its text does not fit it".to_string()],
                guidance: vec![
                    "A catalog act needs its short, plain text; a body act carries none outside its own fields."
                        .to_string(),
                ],
                ..base(f, Outcome::Malformed)
            },
            FutureStatus::Unmeasured => FutureReport {
                reasons: vec!["it is neither a body act nor in the signed catalog".to_string()],
                guidance: vec![
                    "That is not a known action. Propose a body act, or one of the catalog's actions."
                        .to_string(),
                ],
                ..base(f, Outcome::Unmeasured)
            },
        })
        .collect()
}
