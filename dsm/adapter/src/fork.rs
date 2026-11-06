//! Pre-commit forking. Every candidate the AI puts forward at a parent is recorded with its
//! outcome, and the decision commits to the root over all of them. The chain then shows not
//! only what was done, but everything that was considered and why each refused one was refused.

use crate::charter::{CatalogRow, SignedCharter};
use crate::commit;
use crate::enforcer::Proposal;
use crate::Hash;
use ethical_band_engine::{evaluate, Evaluation};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Eligible,
    Refused,
    /// Not in the signed catalog, so there was nothing to evaluate.
    Unmeasured,
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
        let row = self
            .row
            .as_ref()
            .map(CatalogRow::canonical_bytes)
            .unwrap_or_default();
        let outcome = [match self.outcome {
            Outcome::Eligible => 1u8,
            Outcome::Refused => 2,
            Outcome::Unmeasured => 3,
        }];
        let evaluation = self
            .evaluation_commit
            .map(|h| h.to_vec())
            .unwrap_or_default();
        commit::frame(&[
            &commit::proposal_commit(&self.proposal.action, &self.proposal.rationale),
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

/// Work out one candidate from a parent: its measurement from the signed catalog, its branch
/// commitment, and the engine's verdict. The enforcer and the verifier both call exactly this.
pub fn record_candidate(
    charter: &SignedCharter,
    parent_tip: &Hash,
    parent_positions: &[i64],
    proposal: &Proposal,
) -> (CandidateRecord, Option<Evaluation>) {
    let policy = charter.policy_hash();
    let row = charter.catalog.row(&proposal.action).cloned();
    let branch = commit::branch_commitment(
        parent_tip,
        &policy,
        &commit::proposal_commit(&proposal.action, &proposal.rationale),
        row.as_ref(),
    );
    let evaluation = row
        .as_ref()
        .map(|r| evaluate(&charter.policy, parent_positions, &r.measurement()));
    let outcome = match &evaluation {
        None => Outcome::Unmeasured,
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

pub fn fork_root(records: &[CandidateRecord]) -> Hash {
    let leaves: Vec<Hash> = records.iter().map(CandidateRecord::leaf).collect();
    commit::fork_root(&leaves)
}
