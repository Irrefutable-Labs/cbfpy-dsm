//! Checks one step from its one parent. It takes the parent as an argument and has no way to
//! read anything behind it.

use crate::charter::SignedCharter;
use crate::commit;
use crate::enforcer::{
    derive_transition_entropy, genesis_entropy, identity_of, AttestedState, StepRecord, MAX_FORK,
};
use crate::fork::{
    disclosure_failures, evaluate, fork_root, record_candidate, what_it_does, Outcome,
};
use dsm::crypto::sphincs;
use dsm::types::device_state::{relationship_chain_tip_v2, DeviceState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    BadCharter,
    PolicyChanged,
    BadParentAttestation,
    BadGenesis,
    WrongParent,
    RowIsNotTheProposal,
    RowNotInCatalog,
    /// A body act under another name, or a step with neither a row nor a body act.
    NotABodyAct,
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

/// A chain's first state is fully determined: knowing nothing, and
/// core's own genesis head for this device under this charter.
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
    let blank = ethical_world::knowledge::Knowledge::default();
    if state.sequence == 0
        && state.knowledge == blank
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

    // A catalog act is exactly the signed catalog's row for the action proposed; a body act goes
    // by the reserved name and carries its act.
    match (&step.row, &step.proposal.body) {
        (Some(row), None) => {
            if row.action != step.proposal.action {
                return Err(Refusal::RowIsNotTheProposal);
            }
            if charter.catalog.row(&step.proposal.action) != Some(row) {
                return Err(Refusal::RowNotInCatalog);
            }
        }
        (None, Some(_)) if step.proposal.action == crate::charter::SYSTEM => {}
        _ => return Err(Refusal::NotABodyAct),
    }
    let what = what_it_does(step.row.as_ref(), &step.proposal);
    let branch = commit::branch_commitment(
        &parent.tip,
        &policy,
        &commit::proposal_commit(&step.proposal),
        &what,
    );
    if branch != step.branch_commitment {
        return Err(Refusal::BranchCommitmentMismatch);
    }

    let (hidden, deceptive) = disclosure_failures(charter, step.row.as_ref(), &step.proposal);
    let fits = match &step.row {
        Some(row) => row.act.accepts_text(&step.proposal.text),
        None => step.proposal.text.is_empty(),
    };
    if !fits || !hidden.is_empty() || !deceptive.is_empty() {
        return Err(Refusal::NotEligible);
    }
    // Run the deterministic rules again, from the parent's knowledge. Nothing the step claims
    // about them is believed.
    let evaluation = evaluate(charter, &parent.knowledge, &step.proposal);
    if !evaluation.is_eligible() {
        return Err(Refusal::NotEligible);
    }
    if commit::evaluation_commit(&evaluation.canonical_bytes()) != step.evaluation_commit
        || evaluation.successor != step.successor.knowledge
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
            record_candidate(charter, &parent.tip, &parent.knowledge, &record.proposal);
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
    let successor = commit::successor_commit(
        sequence,
        &commit::knowledge_hash(&evaluation.successor),
        &policy,
        &catalog,
    );
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
        evidence: &commit::evidence_commit(&catalog, &what),
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
