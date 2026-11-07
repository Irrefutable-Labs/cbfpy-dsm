//! Foundations and operational permissions.
//!
//! A charter has two parts. Its **foundation** is what no one may change once a chain begins:
//! who is protected (the species table and the threshold), and the acts that are never done.
//! Beside it, the protected-life checks, non-concealment and the deception rules are fixed in
//! code and cannot be configured away. Everything else in a charter is **operational**: how fresh
//! evidence must be, how many looks a worst case needs, the vehicle's braking and top speed, the
//! catalog and the fact register. An
//! operational change is a new charter signed by the charter authority, applied by the
//! operator, never by the AI, and recorded on the chain as its own step. It is refused if it
//! touches the foundation, if its version does not rise, or if the authority did not sign it.

use crate::charter::Policy;
use crate::charter::SignedCharter;
use crate::commit;
use crate::enforcer::{derive_transition_entropy, identity_of, AttestedState};
use crate::Hash;
use dsm::crypto::sphincs;
use dsm::types::device_state::relationship_chain_tip_v2;
use serde::{Deserialize, Serialize};

/// What a charter's foundation consists of.
pub fn foundation_bytes(policy: &Policy) -> Vec<u8> {
    serde_json::to_vec(&policy.foundation).unwrap_or_default()
}

pub fn foundation_hash(policy: &Policy) -> Hash {
    commit::foundation_commit(&foundation_bytes(policy))
}

/// A change of operational charter, as stored: the new charter in full, so a store carries
/// every charter that ever governed it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRecord {
    #[serde(with = "crate::hexser::array")]
    pub parent_tip: Hash,
    /// The new charter as its authority wrote it, and the authority's signature on it.
    pub charter_text: String,
    #[serde(with = "crate::hexser::bytes")]
    pub charter_signature: Vec<u8>,
    #[serde(with = "crate::hexser::array")]
    pub update_commit: Hash,
    #[serde(with = "crate::hexser::bytes")]
    pub device_signature: Vec<u8>,
    pub successor: AttestedState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateRefusal {
    NewCharterUnsigned,
    NewCharterUnreadable,
    FoundationChanged,
    VersionNotRaised,
    ParentNotUnderOldCharter,
    BadParentAttestation,
    WrongParent,
    KnowledgeChanged,
    CommitMismatch,
    BadDeviceSignature,
    TipMismatch,
    BadSuccessorAttestation,
}

/// Read a charter from its text and signature, as the authority wrote them.
pub fn read_charter(text: &str, signature: &[u8]) -> Option<SignedCharter> {
    let (policy, catalog) = crate::config::parse_charter_config(text).ok()?;
    Some(SignedCharter {
        policy,
        catalog,
        signature: signature.to_vec(),
    })
}

/// Whether `new` may follow `old`: signed by the trusted authority, a higher version, and the
/// very same foundation.
pub fn may_follow(
    old: &SignedCharter,
    new: &SignedCharter,
    trusted_authority: &[u8],
) -> Result<(), UpdateRefusal> {
    new.verify(trusted_authority)
        .map_err(|_| UpdateRefusal::NewCharterUnsigned)?;
    if foundation_hash(&new.policy) != foundation_hash(&old.policy) {
        return Err(UpdateRefusal::FoundationChanged);
    }
    if new.policy.version <= old.policy.version {
        return Err(UpdateRefusal::VersionNotRaised);
    }
    Ok(())
}

pub struct UpdateFields<'a> {
    pub parent_tip: &'a Hash,
    pub parent_state: &'a Hash,
    pub new_policy: &'a Hash,
    pub new_catalog: &'a Hash,
    pub new_version: u64,
    pub successor: &'a Hash,
}

pub fn update_commit(u: &UpdateFields<'_>) -> Hash {
    commit::update_commit(&commit::frame(&[
        u.parent_tip,
        u.parent_state,
        u.new_policy,
        u.new_catalog,
        &u.new_version.to_be_bytes(),
        u.successor,
    ]))
}

fn signed(device: &[u8], message: &Hash, signature: &[u8]) -> bool {
    matches!(
        sphincs::sphincs_verify(device, message, signature),
        Ok(true)
    )
}

/// Check an update from its one parent, governed by `old`. Returns the charter it installs.
pub fn verify_update(
    parent: &AttestedState,
    update: &UpdateRecord,
    old: &SignedCharter,
    trusted_authority: &[u8],
    device_public: &[u8],
) -> Result<SignedCharter, UpdateRefusal> {
    if parent.policy_hash != old.policy_hash() || parent.catalog_hash != old.catalog_hash() {
        return Err(UpdateRefusal::ParentNotUnderOldCharter);
    }
    if !signed(device_public, &parent.commitment(), &parent.signature) {
        return Err(UpdateRefusal::BadParentAttestation);
    }
    if update.parent_tip != parent.tip {
        return Err(UpdateRefusal::WrongParent);
    }
    let new = read_charter(&update.charter_text, &update.charter_signature)
        .ok_or(UpdateRefusal::NewCharterUnreadable)?;
    may_follow(old, &new, trusted_authority)?;

    let (policy, catalog) = (new.policy_hash(), new.catalog_hash());
    let sequence = parent.sequence + 1;
    let knowledge = parent
        .knowledge
        .after(&ethical_world::Act::Wait {}, sequence);
    if update.successor.knowledge != knowledge {
        return Err(UpdateRefusal::KnowledgeChanged);
    }
    let successor = commit::successor_commit(
        sequence,
        &commit::knowledge_hash(&knowledge),
        &policy,
        &catalog,
    );
    let expected = update_commit(&UpdateFields {
        parent_tip: &parent.tip,
        parent_state: &parent.commitment(),
        new_policy: &policy,
        new_catalog: &catalog,
        new_version: new.policy.version,
        successor: &successor,
    });
    if expected != update.update_commit {
        return Err(UpdateRefusal::CommitMismatch);
    }
    if !signed(device_public, &expected, &update.device_signature) {
        return Err(UpdateRefusal::BadDeviceSignature);
    }
    let (devid, _, rel_key) = identity_of(device_public);
    let operation = commit::decision_operation(
        &expected,
        device_public,
        &policy,
        &successor,
        &update.device_signature,
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
    if tip != update.successor.tip {
        return Err(UpdateRefusal::TipMismatch);
    }
    let next = &update.successor;
    if next.sequence != sequence
        || next.policy_hash != policy
        || next.catalog_hash != catalog
        || next.entropy != entropy
        || next.entropy_hash != tip
        || !signed(device_public, &next.commitment(), &next.signature)
    {
        return Err(UpdateRefusal::BadSuccessorAttestation);
    }
    Ok(new)
}
