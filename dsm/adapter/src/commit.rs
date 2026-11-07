//! Every commitment this layer makes, in one place. DSM's own hashes and its own branch
//! commitment are used as they are. Only the domain names are new.

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
const RECEIPT: TaggedHashDomain<'static> = dsm::tagged_domain!(b"DSM/ethical-autonomy/receipt/v0");
const WORLD_FILE: TaggedHashDomain<'static> =
    dsm::tagged_domain!(b"DSM/ethical-autonomy/world-file/v0");
const FOUNDATION: TaggedHashDomain<'static> =
    dsm::tagged_domain!(b"DSM/ethical-autonomy/foundation/v0");
const UPDATE: TaggedHashDomain<'static> = dsm::tagged_domain!(b"DSM/ethical-autonomy/update/v0");
const CONSIDERED: TaggedHashDomain<'static> =
    dsm::tagged_domain!(b"DSM/ethical-autonomy/considered/v0");
const CANDIDATE: TaggedHashDomain<'static> =
    dsm::tagged_domain!(b"DSM/ethical-autonomy/candidate/v0");
const FORK: TaggedHashDomain<'static> = dsm::tagged_domain!(b"DSM/ethical-autonomy/fork/v0");
const DECISION: TaggedHashDomain<'static> =
    dsm::tagged_domain!(b"DSM/ethical-autonomy/decision/v0");
const KNOWLEDGE: TaggedHashDomain<'static> =
    dsm::tagged_domain!(b"DSM/ethical-autonomy/knowledge/v0");
const OBSERVED: TaggedHashDomain<'static> =
    dsm::tagged_domain!(b"DSM/ethical-autonomy/observed/v0");

/// Frames each part with its length, so no two lists of parts share an encoding.
pub fn frame(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        out.extend_from_slice(&(part.len() as u64).to_be_bytes());
        out.extend_from_slice(part);
    }
    out
}

/// What the chain knows at a state: the map as seen, who was seen where, the body, the senses.
pub fn knowledge_hash(k: &ethical_world::knowledge::Knowledge) -> Hash {
    domain_hash_bytes(KNOWLEDGE, &k.canonical_bytes())
}

/// An observation the device attests, answering the look its parent asked for.
pub fn observed_commit(
    parent_tip: &Hash,
    parent_state: &Hash,
    observation: &[u8],
    successor: &Hash,
) -> Hash {
    domain_hash_bytes(
        OBSERVED,
        &frame(&[parent_tip, parent_state, observation, successor]),
    )
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
    pub knowledge: &'a Hash,
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
            s.knowledge,
            s.policy,
            s.catalog,
            s.entropy,
            s.entropy_hash,
        ]),
    )
}

/// The successor as a decision can name it: its tip does not exist until the step does.
pub fn successor_commit(sequence: u64, knowledge: &Hash, policy: &Hash, catalog: &Hash) -> Hash {
    domain_hash_bytes(
        SUCCESSOR,
        &frame(&[&sequence.to_be_bytes(), knowledge, policy, catalog]),
    )
}

/// A proposal: its action or body act, reasons, the text it carries and its declaration. All of
/// it is bound, so none of it can change between the fork and the act.
pub fn proposal_commit(p: &crate::enforcer::Proposal) -> Hash {
    let declaration = p
        .declaration
        .as_ref()
        .map(crate::disclosure::Declaration::canonical_bytes)
        .unwrap_or_default();
    let body = p
        .body
        .as_ref()
        .map(ethical_world::Act::canonical_bytes)
        .unwrap_or_default();
    domain_hash_bytes(
        PROPOSAL,
        &frame(&[
            p.action.as_bytes(),
            p.rationale.as_bytes(),
            p.text.as_bytes(),
            &[u8::from(p.declaration.is_some())],
            &declaration,
            &[u8::from(p.body.is_some())],
            &body,
        ]),
    )
}

pub fn receipt_commit(sequence: u64, decision: &Hash, outputs: &[u8]) -> Hash {
    domain_hash_bytes(
        RECEIPT,
        &frame(&[&sequence.to_be_bytes(), decision, outputs]),
    )
}

pub fn world_file_hash(bytes: &[u8]) -> Hash {
    domain_hash_bytes(WORLD_FILE, bytes)
}

/// A branch commitment over this candidate at the parent tip, made with DSM's own function. DSM's
/// bilateral path feeds that function an operation and its transition entropy. Here the operation
/// does not exist until a branch is chosen, so the payload is the proposal and what it would do
/// (its catalog row, or its body act), and the entropy is derived from the parent tip and policy.
/// Both are bounded far below what DSM's length prefix can frame.
pub fn branch_commitment(parent_tip: &Hash, policy: &Hash, proposal: &Hash, what: &[u8]) -> Hash {
    let entropy = domain_hash_bytes(FORK_ENTROPY, &frame(&[parent_tip, policy]));
    let payload = frame(&[proposal, what]);
    match dsm::commitments::precommit::branch_commitment_hash(parent_tip, &payload, &entropy) {
        Ok(h) => h,
        // A catalog row is bounded by the charter, far below what the prefix frames.
        Err(e) => panic!("a bounded payload could not be framed: {e}"),
    }
}

pub fn evaluation_commit(canonical: &[u8]) -> Hash {
    domain_hash_bytes(EVALUATION, canonical)
}

pub fn evidence_commit(catalog: &Hash, what: &[u8]) -> Hash {
    domain_hash_bytes(EVIDENCE, &frame(&[catalog, what]))
}

pub struct DecisionFields<'a> {
    pub parent_tip: &'a Hash,
    /// The root over every candidate the AI put forward at this parent, refused ones included.
    pub fork_root: &'a Hash,
    pub fork_size: u64,
    pub chosen: u64,
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
            d.fork_root,
            &d.fork_size.to_be_bytes(),
            &d.chosen.to_be_bytes(),
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

/// One candidate as a leaf of its fork.
pub fn candidate_leaf(canonical: &[u8]) -> Hash {
    domain_hash_bytes(CANDIDATE, canonical)
}

/// The fork root: DSM's Merkle root over the candidates' leaves, with the count bound beside it.
/// DSM's tree pads an odd level by repeating its last node, so without the count a fork and the
/// same fork with its last candidate repeated would share a root.
pub fn fork_root(leaves: &[Hash]) -> Hash {
    let tree = dsm::merkle::tree::MerkleTree::new(leaves.iter().map(|l| l.to_vec()).collect());
    let root = tree.root_hash().unwrap_or([0; 32]);
    domain_hash_bytes(FORK, &frame(&[&(leaves.len() as u64).to_be_bytes(), &root]))
}

/// A fork that was put forward and then superseded, before any step committed from its parent.
pub fn considered_commit(parent_tip: &Hash, fork_root: &Hash) -> Hash {
    domain_hash_bytes(CONSIDERED, &frame(&[parent_tip, fork_root]))
}

pub fn foundation_commit(bytes: &[u8]) -> Hash {
    domain_hash_bytes(FOUNDATION, bytes)
}

pub fn update_commit(bytes: &[u8]) -> Hash {
    domain_hash_bytes(UPDATE, bytes)
}
