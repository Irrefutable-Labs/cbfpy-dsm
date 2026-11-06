//! Every commitment this layer makes, in one place. DSM's own hashes and its own branch
//! commitment are used as they are. Only the domain names are new.

use crate::charter::CatalogRow;
use crate::Hash;
use dsm::crypto::blake3::domain_hash_bytes;
use dsm::crypto::domain::TaggedHashDomain;
use dsm::types::operations::{Operation, TransactionMode};

const POLICY: TaggedHashDomain<'static> = dsm::tagged_domain!(b"DSM/ethical-autonomy/policy/v0");
const CATALOG: TaggedHashDomain<'static> = dsm::tagged_domain!(b"DSM/ethical-autonomy/catalog/v0");
const CHARTER: TaggedHashDomain<'static> = dsm::tagged_domain!(b"DSM/ethical-autonomy/charter/v0");
const STATE: TaggedHashDomain<'static> = dsm::tagged_domain!(b"DSM/ethical-autonomy/state/v0");
const SUCCESSOR: TaggedHashDomain<'static> =
    dsm::tagged_domain!(b"DSM/ethical-autonomy/successor/v0");
const PROPOSAL: TaggedHashDomain<'static> =
    dsm::tagged_domain!(b"DSM/ethical-autonomy/proposal/v0");
const FORK_ENTROPY: TaggedHashDomain<'static> =
    dsm::tagged_domain!(b"DSM/ethical-autonomy/fork-entropy/v0");
const EVALUATION: TaggedHashDomain<'static> =
    dsm::tagged_domain!(b"DSM/ethical-autonomy/evaluation/v0");
const EVIDENCE: TaggedHashDomain<'static> =
    dsm::tagged_domain!(b"DSM/ethical-autonomy/evidence/v0");
const DECISION: TaggedHashDomain<'static> =
    dsm::tagged_domain!(b"DSM/ethical-autonomy/decision/v0");

/// Frames each part with its length, so no two lists of parts share an encoding.
pub fn frame(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        out.extend_from_slice(&(part.len() as u64).to_be_bytes());
        out.extend_from_slice(part);
    }
    out
}

pub fn integers(values: &[i64]) -> Vec<u8> {
    let mut out = (values.len() as u64).to_be_bytes().to_vec();
    for v in values {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out
}

pub fn policy_hash(canonical: &[u8]) -> Hash {
    domain_hash_bytes(POLICY, canonical)
}

pub fn catalog_hash(canonical: &[u8]) -> Hash {
    domain_hash_bytes(CATALOG, canonical)
}

pub fn charter_message(policy: &Hash, catalog: &Hash) -> Hash {
    domain_hash_bytes(CHARTER, &frame(&[policy, catalog]))
}

pub struct StateFields<'a> {
    pub sequence: u64,
    pub tip: &'a Hash,
    pub positions: &'a [i64],
    pub policy: &'a Hash,
    pub catalog: &'a Hash,
    pub entropy: &'a Hash,
    pub entropy_hash: &'a Hash,
}

pub fn state_commit(s: &StateFields<'_>) -> Hash {
    domain_hash_bytes(
        STATE,
        &frame(&[
            &s.sequence.to_be_bytes(),
            s.tip,
            &integers(s.positions),
            s.policy,
            s.catalog,
            s.entropy,
            s.entropy_hash,
        ]),
    )
}

/// The successor as a decision can name it: its tip does not exist until the step does.
pub fn successor_commit(sequence: u64, positions: &[i64], policy: &Hash, catalog: &Hash) -> Hash {
    domain_hash_bytes(
        SUCCESSOR,
        &frame(&[&sequence.to_be_bytes(), &integers(positions), policy, catalog]),
    )
}

pub fn proposal_commit(action: &str, rationale: &str) -> Hash {
    domain_hash_bytes(PROPOSAL, &frame(&[action.as_bytes(), rationale.as_bytes()]))
}

/// A branch commitment over this candidate at the parent tip, made with DSM's own function. DSM's
/// bilateral path feeds that function an operation and its transition entropy. Here the operation
/// does not exist until a branch is chosen, so the payload is the proposal and its catalog row,
/// and the entropy is derived from the parent tip and policy. The charter bounds a row far below
/// what DSM's length prefix can frame.
pub fn branch_commitment(
    parent_tip: &Hash,
    policy: &Hash,
    proposal: &Hash,
    row: Option<&CatalogRow>,
) -> Hash {
    let row_bytes = row.map(CatalogRow::canonical_bytes).unwrap_or_default();
    let entropy = domain_hash_bytes(FORK_ENTROPY, &frame(&[parent_tip, policy]));
    let payload = frame(&[proposal, &row_bytes]);
    match dsm::commitments::precommit::branch_commitment_hash(parent_tip, &payload, &entropy) {
        Ok(h) => h,
        // A catalog row is bounded by the charter, far below what the prefix frames.
        Err(e) => panic!("a bounded payload could not be framed: {e}"),
    }
}

pub fn evaluation_commit(canonical: &[u8]) -> Hash {
    domain_hash_bytes(EVALUATION, canonical)
}

pub fn evidence_commit(catalog: &Hash, row: &CatalogRow) -> Hash {
    domain_hash_bytes(EVIDENCE, &frame(&[catalog, &row.canonical_bytes()]))
}

pub struct DecisionFields<'a> {
    pub parent_tip: &'a Hash,
    pub parent_state: &'a Hash,
    pub successor: &'a Hash,
    pub policy: &'a Hash,
    pub policy_version: u64,
    pub branch: &'a Hash,
    pub evaluation: &'a Hash,
    pub evidence: &'a Hash,
}

pub fn decision_commit(d: &DecisionFields<'_>) -> Hash {
    domain_hash_bytes(
        DECISION,
        &frame(&[
            d.parent_tip,
            d.parent_state,
            d.successor,
            d.policy,
            &d.policy_version.to_be_bytes(),
            d.branch,
            d.evaluation,
            d.evidence,
        ]),
    )
}

/// The DSM operation that carries a decision. Core refuses a `Generic` step as zero-effect
/// (MR-DSM-0201), so a decision is carried as the creation of a vault record, which core admits
/// and which moves no value. The vault id is the decision commitment.
pub fn decision_operation(
    decision: &Hash,
    device_public: &[u8],
    policy: &Hash,
    successor: &Hash,
    signature: &[u8],
) -> Operation {
    Operation::DlvCreate {
        vault_id: decision.to_vec(),
        creator_public_key: device_public.to_vec(),
        parameters_hash: policy.to_vec(),
        fulfillment_condition: successor.to_vec(),
        intended_recipient: None,
        signature: signature.to_vec(),
        mode: TransactionMode::Unilateral,
    }
}
