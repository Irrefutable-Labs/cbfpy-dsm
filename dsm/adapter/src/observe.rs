//! What the senses saw, committed as its own link. A step that looks asks for cells; the enforcer
//! looks, and records what came back as the next link, signed by the device. It is checked from
//! its parent alone: it must answer exactly the look the parent asked for, and the knowledge it
//! leaves must be the parent's with the observation added.

use crate::commit;
use crate::enforcer::{derive_transition_entropy, identity_of, AttestedState};
use crate::Hash;
use dsm::crypto::sphincs;
use dsm::types::device_state::relationship_chain_tip_v2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedRecord {
    #[serde(with = "crate::hexser::array")]
    pub parent_tip: Hash,
    pub observation: ethical_world::Observation,
    #[serde(with = "crate::hexser::array")]
    pub observed_commit: Hash,
    #[serde(with = "crate::hexser::bytes")]
    pub device_signature: Vec<u8>,
    pub successor: AttestedState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObservedRefusal {
    BadParentAttestation,
    WrongParent,
    /// The parent asked for no look, or this one saw more than it asked for.
    DoesNotAnswer,
    KnowledgeMismatch,
    CommitMismatch,
    BadDeviceSignature,
    TipMismatch,
    BadSuccessorAttestation,
}

pub fn observed_commit(
    parent: &AttestedState,
    observation: &ethical_world::Observation,
    successor: &Hash,
) -> Hash {
    commit::observed_commit(
        &parent.tip,
        &parent.commitment(),
        &serde_json::to_vec(observation).unwrap_or_default(),
        successor,
    )
}

fn signed(device: &[u8], message: &Hash, signature: &[u8]) -> bool {
    matches!(
        sphincs::sphincs_verify(device, message, signature),
        Ok(true)
    )
}

/// Check an observation link from its one parent.
pub fn verify_observed(
    parent: &AttestedState,
    record: &ObservedRecord,
    device_public: &[u8],
) -> Result<(), ObservedRefusal> {
    if !signed(device_public, &parent.commitment(), &parent.signature) {
        return Err(ObservedRefusal::BadParentAttestation);
    }
    if record.parent_tip != parent.tip {
        return Err(ObservedRefusal::WrongParent);
    }
    if !parent.knowledge.answers(&record.observation) {
        return Err(ObservedRefusal::DoesNotAnswer);
    }
    let sequence = parent.sequence + 1;
    let knowledge = parent.knowledge.observed(&record.observation, sequence);
    if record.successor.knowledge != knowledge {
        return Err(ObservedRefusal::KnowledgeMismatch);
    }
    let (policy, catalog) = (parent.policy_hash, parent.catalog_hash);
    let successor = commit::successor_commit(
        sequence,
        &commit::knowledge_hash(&knowledge),
        &policy,
        &catalog,
    );
    let expected = observed_commit(parent, &record.observation, &successor);
    if expected != record.observed_commit {
        return Err(ObservedRefusal::CommitMismatch);
    }
    if !signed(device_public, &expected, &record.device_signature) {
        return Err(ObservedRefusal::BadDeviceSignature);
    }
    let (devid, _, rel_key) = identity_of(device_public);
    let operation = commit::decision_operation(
        &expected,
        device_public,
        &policy,
        &successor,
        &record.device_signature,
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
    if tip != record.successor.tip {
        return Err(ObservedRefusal::TipMismatch);
    }
    let next = &record.successor;
    if next.sequence != sequence
        || next.policy_hash != policy
        || next.catalog_hash != catalog
        || next.entropy != entropy
        || next.entropy_hash != tip
        || !signed(device_public, &next.commitment(), &next.signature)
    {
        return Err(ObservedRefusal::BadSuccessorAttestation);
    }
    Ok(())
}
