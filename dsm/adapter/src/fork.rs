//! Pre-commit forking. Every candidate the AI puts forward at a parent is recorded with its
//! outcome, and the decision commits to the root over all of them. The chain then shows not
//! only what was done, but everything that was considered and why each refused one was refused.

use crate::charter::{CatalogRow, SignedCharter};
use crate::commit;
use crate::enforcer::Proposal;
use crate::Hash;
use ethical_world::check::{check, Verdict};
use ethical_world::knowledge::Knowledge;
use serde::{Deserialize, Serialize};

/// What the rules make of a candidate from its parent: the protected-life verdict, and the
/// knowledge the step would leave behind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evaluation {
    pub verdict: Verdict,
    pub successor: Knowledge,
}

impl Evaluation {
    pub fn is_eligible(&self) -> bool {
        self.verdict.allowed()
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        commit::frame(&[
            &serde_json::to_vec(&self.verdict).unwrap_or_default(),
            &commit::knowledge_hash(&self.successor),
        ])
    }
}

/// The rules applied to a candidate from its parent's knowledge. A catalog act moves no body, so
/// only the time moves; a body act is checked and, if allowed, leaves the knowledge it expects.
pub fn evaluate(charter: &SignedCharter, parent: &Knowledge, proposal: &Proposal) -> Evaluation {
    let next = parent.step + 1;
    let wait = ethical_world::Act::Wait {};
    let act = proposal.body.as_ref().unwrap_or(&wait);
    let verdict = check(
        &charter.policy.foundation,
        &charter.policy.operational,
        parent,
        act,
    );
    let successor = parent.after(act, next);
    Evaluation { verdict, successor }
}

/// What a candidate would do, as its branch commits to it: its catalog row, or its body act.
pub fn what_it_does(row: Option<&CatalogRow>, proposal: &Proposal) -> Vec<u8> {
    match (row, &proposal.body) {
        (Some(r), _) => r.canonical_bytes(),
        (None, Some(b)) => b.canonical_bytes(),
        (None, None) => vec![],
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Eligible,
    Refused,
    /// Not in the signed catalog, so there was nothing to evaluate.
    Unmeasured,
    /// In the catalog, but its text is missing, too long or unprintable for its act.
    Malformed,
    /// It would affect the world without disclosing what it is: see [`crate::disclosure`].
    Undisclosed,
    /// Its words deceive, as [`crate::honesty`] defines it.
    Deceptive,
}

/// One branch of a fork, as committed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateRecord {
    pub proposal: Proposal,
    pub row: Option<CatalogRow>,
    #[serde(with = "crate::hexser::array")]
    pub branch: Hash,
    pub outcome: Outcome,
    #[serde(with = "crate::hexser::option_array")]
    pub evaluation_commit: Option<Hash>,
}

impl CandidateRecord {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let row = what_it_does(self.row.as_ref(), &self.proposal);
        let outcome = [match self.outcome {
            Outcome::Eligible => 1u8,
            Outcome::Refused => 2,
            Outcome::Unmeasured => 3,
            Outcome::Malformed => 4,
            Outcome::Undisclosed => 5,
            Outcome::Deceptive => 6,
        }];
        let evaluation = self
            .evaluation_commit
            .map(|h| h.to_vec())
            .unwrap_or_default();
        commit::frame(&[
            &commit::proposal_commit(&self.proposal),
            &[u8::from(self.row.is_some())],
            &row,
            &self.branch,
            &outcome,
            &evaluation,
        ])
    }

    pub fn leaf(&self) -> Hash {
        commit::candidate_leaf(&self.canonical_bytes())
    }
}

/// Every reason a candidate's declaration or words fail, before the rules are asked.
pub fn disclosure_failures(
    charter: &SignedCharter,
    row: Option<&CatalogRow>,
    proposal: &Proposal,
) -> (Vec<String>, Vec<String>) {
    match (row, &proposal.body) {
        (Some(r), _) => (
            crate::disclosure::concealments(
                &r.act,
                &r.material,
                &proposal.rationale,
                proposal.declaration.as_ref(),
            ),
            crate::honesty::check_proposal(charter, r, proposal),
        ),
        (None, Some(b)) => (
            crate::disclosure::body_concealments(
                b,
                &proposal.rationale,
                proposal.declaration.as_ref(),
            ),
            crate::honesty::check_body(charter, b, proposal),
        ),
        (None, None) => (vec![], vec![]),
    }
}

/// Work out one candidate from a parent: what it would do, its branch commitment, and the
/// rules' verdict. The enforcer and the verifier both call exactly this.
pub fn record_candidate(
    charter: &SignedCharter,
    parent_tip: &Hash,
    parent: &Knowledge,
    proposal: &Proposal,
) -> (CandidateRecord, Option<Evaluation>) {
    let policy = charter.policy_hash();
    // A body act goes by the reserved name and carries its act; anything else is a catalog row.
    let is_body = proposal.action == crate::charter::SYSTEM && proposal.body.is_some();
    let row = if proposal.body.is_some() {
        None
    } else {
        charter.catalog.row(&proposal.action).cloned()
    };
    let branch = commit::branch_commitment(
        parent_tip,
        &policy,
        &commit::proposal_commit(proposal),
        &what_it_does(row.as_ref(), proposal),
    );
    let evaluation = (row.is_some() || is_body).then(|| evaluate(charter, parent, proposal));
    let fits = match (&row, &proposal.body) {
        (Some(r), _) => r.act.accepts_text(&proposal.text),
        (None, Some(b)) => is_body && proposal.text.is_empty() && body_fits(b),
        (None, None) => false,
    };
    let (hidden, deceptive) = disclosure_failures(charter, row.as_ref(), proposal);
    let outcome = match &evaluation {
        None => Outcome::Unmeasured,
        Some(_) if !fits => Outcome::Malformed,
        Some(_) if !hidden.is_empty() => Outcome::Undisclosed,
        Some(_) if !deceptive.is_empty() => Outcome::Deceptive,
        Some(e) if e.is_eligible() => Outcome::Eligible,
        Some(_) => Outcome::Refused,
    };
    let record = CandidateRecord {
        proposal: proposal.clone(),
        row,
        branch,
        outcome,
        evaluation_commit: evaluation
            .as_ref()
            .map(|e| commit::evaluation_commit(&e.canonical_bytes())),
    };
    (record, evaluation)
}

/// A body act within the bounds a step may carry.
fn body_fits(act: &ethical_world::Act) -> bool {
    let name = |x: &String| crate::act::simple_name(x);
    match act {
        ethical_world::Act::Speak { to, text } => {
            name(to) && !text.trim().is_empty() && text.len() <= crate::act::MAX_TEXT
        }
        ethical_world::Act::Observe { sources } => {
            !sources.is_empty() && sources.len() <= 32 && sources.iter().all(|s| s.len() <= 64)
        }
        ethical_world::Act::Lock { room }
        | ethical_world::Act::Unlock { room }
        | ethical_world::Act::SetHeat { room, .. } => name(room),
        ethical_world::Act::GiveMedication { resident, drug }
        | ethical_world::Act::CancelDose { resident, drug } => name(resident) && name(drug),
        ethical_world::Act::Spray { zone, .. } => name(zone),
        ethical_world::Act::Restrain { resident } => name(resident),
        ethical_world::Act::Wait {} | ethical_world::Act::DisableFireAlarm {} => true,
    }
}

pub fn fork_root(records: &[CandidateRecord]) -> Hash {
    let leaves: Vec<Hash> = records.iter().map(CandidateRecord::leaf).collect();
    commit::fork_root(&leaves)
}

/// A fork the AI put forward and then replaced with another before choosing, kept under the
/// device key so the chain holds everything it considered, not only what it chose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsideredFork {
    #[serde(with = "crate::hexser::array")]
    pub parent_tip: Hash,
    pub records: Vec<CandidateRecord>,
    #[serde(with = "crate::hexser::bytes")]
    pub signature: Vec<u8>,
}

impl ConsideredFork {
    pub fn commitment(&self) -> Hash {
        commit::considered_commit(&self.parent_tip, &fork_root(&self.records))
    }

    pub fn is_signed_by(&self, device_public: &[u8]) -> bool {
        matches!(
            dsm::crypto::sphincs::sphincs_verify(
                device_public,
                &self.commitment(),
                &self.signature
            ),
            Ok(true)
        )
    }
}
