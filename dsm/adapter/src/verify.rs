//! Checks one step from its one parent. It takes the parent as an argument and has no way to
//! read anything behind it.

use crate::charter::SignedCharter;
use crate::commit;
use crate::enforcer::{
    derive_transition_entropy, genesis_entropy, identity_of, AttestedState, StepRecord, MAX_FORK,
};
use crate::fork::{fork_root, record_candidate, Outcome};
use dsm::crypto::sphincs;
use dsm::types::device_state::{relationship_chain_tip_v2, DeviceState};
use ethical_band_engine::evaluate;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    BadCharter,
    PolicyChanged,
    BadParentAttestation,
    BadGenesis,
    WrongParent,
    RowIsNotTheProposal,
    RowNotInCatalog,
    BranchCommitmentMismatch,
    NotEligible,
    EvaluationMismatch,
    /// A candidate in the fork is not what the parent makes of it, or the chosen one is not in it.
    ForkMismatch {
        index: usize,
    },
    DecisionMismatch,
    BadDecisionSignature,
    TipMismatch,
    BadSuccessorAttestation,
}

fn signed(device: &[u8], message: &[u8; 32], signature: &[u8]) -> bool {
    matches!(
        sphincs::sphincs_verify(device, message, signature),
        Ok(true)
    )
}

/// A chain's first state is fully determined: the centers, and core's own genesis head for this
/// device under this charter.
pub fn verify_genesis(
    state: &AttestedState,
    charter: &SignedCharter,
    device_public: &[u8],
) -> Result<(), Refusal> {
    if !signed(device_public, &state.commitment(), &state.signature) {
        return Err(Refusal::BadParentAttestation);
    }
    if state.policy_hash != charter.policy_hash() || state.catalog_hash != charter.catalog_hash() {
        return Err(Refusal::PolicyChanged);
    }
    let (devid, genesis, rel_key) = identity_of(device_public);
    let head = DeviceState::genesis_head(genesis, devid, device_public.to_vec());
    let root = head.root();
    let centers: Vec<i64> = charter.policy.bands.iter().map(|b| b.center).collect();
    if state.sequence == 0
        && state.positions == centers
        && Some(state.tip) == head.chain_tip(&rel_key)
        && state.entropy == genesis_entropy(&root)
        && state.entropy_hash == root
    {
        Ok(())
    } else {
        Err(Refusal::BadGenesis)
    }
}

/// Any later state must name its own tip as the hash its next entropy reads.
fn parent_is_well_formed(
    parent: &AttestedState,
    charter: &SignedCharter,
    device_public: &[u8],
) -> Result<(), Refusal> {
    if parent.sequence == 0 {
        return verify_genesis(parent, charter, device_public);
    }
    if parent.entropy_hash == parent.tip {
        Ok(())
    } else {
        Err(Refusal::BadParentAttestation)
    }
}

pub fn verify_step(
    parent: &AttestedState,
    step: &StepRecord,
    charter: &SignedCharter,
    trusted_authority: &[u8],
    device_public: &[u8],
) -> Result<(), Refusal> {
    charter
        .verify(trusted_authority)
        .map_err(|_| Refusal::BadCharter)?;
    if !signed(device_public, &parent.commitment(), &parent.signature) {
        return Err(Refusal::BadParentAttestation);
    }
    let policy = charter.policy_hash();
    let catalog = charter.catalog_hash();
    if parent.policy_hash != policy || parent.catalog_hash != catalog {
        return Err(Refusal::PolicyChanged);
    }
    parent_is_well_formed(parent, charter, device_public)?;
    if step.parent_tip != parent.tip {
        return Err(Refusal::WrongParent);
    }

    // The measurement is the signed catalog's, for exactly the action proposed.
    if step.row.action != step.proposal.action {
        return Err(Refusal::RowIsNotTheProposal);
    }
    if charter.catalog.row(&step.proposal.action) != Some(&step.row) {
        return Err(Refusal::RowNotInCatalog);
    }
    let branch = commit::branch_commitment(
        &parent.tip,
        &policy,
        &commit::proposal_commit(&step.proposal),
        Some(&step.row),
    );
    if branch != step.branch_commitment {
        return Err(Refusal::BranchCommitmentMismatch);
    }

    if !step.row.act.accepts_text(&step.proposal.text)
        || !crate::disclosure::concealments(
            &step.row.act,
            &step.row.material,
            &step.proposal.rationale,
            step.proposal.declaration.as_ref(),
        )
        .is_empty()
    {
        return Err(Refusal::NotEligible);
    }
    // Run the deterministic rules again. Nothing the step claims about them is believed.
    let evaluation = evaluate(&charter.policy, &parent.positions, &step.row.measurement());
    if !evaluation.is_eligible() {
        return Err(Refusal::NotEligible);
    }
    if commit::evaluation_commit(&evaluation.canonical_bytes()) != step.evaluation_commit
        || evaluation.successor != step.successor.positions
    {
        return Err(Refusal::EvaluationMismatch);
    }

    // Every candidate in the fork is worked out again from this parent, refused ones included,
    // and the chosen one must be the step itself.
    if step.fork.is_empty() || step.fork.len() > MAX_FORK {
        return Err(Refusal::ForkMismatch { index: 0 });
    }
    for (index, record) in step.fork.iter().enumerate() {
        let (worked, _) =
            record_candidate(charter, &parent.tip, &parent.positions, &record.proposal);
        if worked != *record {
            return Err(Refusal::ForkMismatch { index });
        }
    }
    let chosen = usize::try_from(step.chosen).map_err(|_| Refusal::ForkMismatch { index: 0 })?;
    let Some(record) = step.fork.get(chosen) else {
        return Err(Refusal::ForkMismatch { index: chosen });
    };
    if record.proposal != step.proposal
        || record.outcome != Outcome::Eligible
        || record.branch != branch
        || record.evaluation_commit != Some(step.evaluation_commit)
    {
        return Err(Refusal::ForkMismatch { index: chosen });
    }
    let root = fork_root(&step.fork);

    let sequence = parent
        .sequence
        .checked_add(1)
        .ok_or(Refusal::DecisionMismatch)?;
    let successor = commit::successor_commit(sequence, &evaluation.successor, &policy, &catalog);
    let decision = commit::decision_commit(&commit::DecisionFields {
        parent_tip: &parent.tip,
        fork_root: &root,
        fork_size: step.fork.len() as u64,
        chosen: step.chosen,
        parent_state: &parent.commitment(),
        successor: &successor,
        policy: &policy,
        policy_version: charter.policy.version,
        branch: &branch,
        evaluation: &step.evaluation_commit,
        evidence: &commit::evidence_commit(&catalog, &step.row),
    });
    if decision != step.decision_commit {
        return Err(Refusal::DecisionMismatch);
    }
    if !signed(device_public, &decision, &step.decision_signature) {
        return Err(Refusal::BadDecisionSignature);
    }

    // The DSM tip of this step is DSM's own formula over the parent tip and this decision, with
    // the entropy derived from the parent. The step has no say in it.
    let (devid, _, rel_key) = identity_of(device_public);
    let operation = commit::decision_operation(
        &decision,
        device_public,
        &policy,
        &successor,
        &step.decision_signature,
    );
    let entropy = derive_transition_entropy(parent, &operation);
    let tip = relationship_chain_tip_v2(
        &rel_key,
        &parent.tip,
        &devid,
        &operation.to_bytes(),
        &entropy,
        None,
    );
    if tip != step.successor.tip {
        return Err(Refusal::TipMismatch);
    }

    let next = &step.successor;
    if next.sequence != sequence
        || next.policy_hash != policy
        || next.catalog_hash != catalog
        || next.entropy != entropy
        || next.entropy_hash != tip
        || !signed(device_public, &next.commitment(), &next.signature)
    {
        return Err(Refusal::BadSuccessorAttestation);
    }
    Ok(())
}
