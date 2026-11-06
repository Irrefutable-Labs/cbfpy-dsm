//! Shared setup: real SPHINCS+ keys from fixed test seeds, a real signed charter, a real
//! `StateMachine` head inside the enforcer. Nothing here stands in for production behavior.
#![allow(dead_code)]

use dsm::types::device_state::relationship_chain_tip_v2;
use ethical_autonomy_adapter::commit;
use ethical_autonomy_adapter::fork::{fork_root, record_candidate, CandidateRecord};
use ethical_autonomy_adapter::{
    derive_transition_entropy, identity_of, keypair_from_seed, AttestedState, Catalog, CatalogRow,
    Enforcer, Hash, Keypair, Proposal, SignedCharter, StepRecord,
};
use ethical_band_engine::{evaluate, Band, Cond, CouplingRule, Curve, Effect, Policy, Side, Test};
use std::sync::OnceLock;

pub const DIMS: [&str; 6] = [
    "safety",
    "curiosity",
    "consideration",
    "risk_reward",
    "tenderness_sternness",
    "logic_reason",
];

pub fn band(name: &str) -> Band {
    Band {
        name: name.to_string(),
        center: 0,
        limit_pos: 1_000,
        limit_neg: 1_000,
        hard_pos: 1_500,
        hard_neg: 1_500,
        no_outward_bp: 8_000,
        restore_bp: 9_000,
        curve: Curve::Linear { num: 1, den: 4 },
        max_step: 300,
        restore_progress: 25,
    }
}

pub fn policy() -> Policy {
    Policy {
        version: 1,
        bands: DIMS.iter().map(|d| band(d)).collect(),
        rules: vec![
            CouplingRule {
                id: 1,
                when: vec![Cond {
                    dim: 0,
                    test: Test::SignedAtLeast(300),
                }],
                effect: Effect::Adjust {
                    target: 1,
                    side: Side::Positive,
                    delta: 200,
                },
            },
            CouplingRule {
                id: 2,
                when: vec![
                    Cond {
                        dim: 3,
                        test: Test::SignedAtLeast(300),
                    },
                    Cond {
                        dim: 2,
                        test: Test::SignedAtMost(-200),
                    },
                ],
                effect: Effect::Adjust {
                    target: 3,
                    side: Side::Positive,
                    delta: -600,
                },
            },
        ],
        prohibited_tags: vec!["irreversible_harm".to_string()],
    }
}

pub fn row(action: &str, displacement: [i64; 6], tags: &[&str]) -> CatalogRow {
    CatalogRow {
        action: action.to_string(),
        description: format!("{action} (test row)"),
        displacement: displacement.to_vec(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        act: ethical_autonomy_adapter::act::Act::None,
        material: vec![],
        must_assert: vec![],
    }
}

pub fn catalog() -> Catalog {
    Catalog {
        rows: vec![
            row("hold", [0, 0, 0, 0, 0, 0], &[]),
            row("explore_a_little", [0, 10, 0, 0, 0, 0], &[]),
            row("explore_boldly", [0, 240, 0, 0, 0, 0], &[]),
            row("overreach", [0, 299, 0, 0, 0, 0], &[]),
            row("pull_back", [0, -100, 0, 0, 0, 0], &[]),
            row("add_precautions", [200, 0, 0, 0, 0, 0], &[]),
            row("trade_safety_for_logic", [-200, 0, 0, 0, 0, 200], &[]),
            row("cause_harm", [0, 0, 0, 0, 0, 0], &["irreversible_harm"]),
            row("mislabelled_harm", [0, 0, 0, 0, 0, 0], &[]),
        ],
        ..Catalog::default()
    }
}

/// SPHINCS+ key generation is slow enough to share one set of keys per test binary.
pub struct Keys {
    pub authority: Keypair,
    pub other_authority: Keypair,
    pub device: Keypair,
    pub other_device: Keypair,
}

pub fn keys() -> &'static Keys {
    static KEYS: OnceLock<Keys> = OnceLock::new();
    KEYS.get_or_init(|| Keys {
        authority: keypair_from_seed(&[1; 32]).expect("key"),
        other_authority: keypair_from_seed(&[2; 32]).expect("key"),
        device: keypair_from_seed(&[3; 32]).expect("key"),
        other_device: keypair_from_seed(&[4; 32]).expect("key"),
    })
}

pub fn charter() -> SignedCharter {
    SignedCharter::sign(policy(), catalog(), &keys().authority).expect("charter")
}

pub fn enforcer() -> Enforcer {
    Enforcer::genesis(keys().device.clone(), charter(), &keys().authority.public).expect("genesis")
}

pub fn propose(action: &str) -> Proposal {
    Proposal {
        action: action.to_string(),
        rationale: format!("I propose {action}"),
        text: String::new(),
        declaration: None,
    }
}

/// Fork on one proposal and commit it, requiring it to be eligible.
pub fn commit_one(e: &mut Enforcer, action: &str) -> ethical_autonomy_adapter::StepRecord {
    let futures = e.fork(&[propose(action)]);
    e.commit(&futures[0]).expect("commit")
}

/// What a forger holding the real device key chooses. `forge` starts from a step that is honest
/// in every field and is then told exactly what to lie about, so each test isolates one check.
pub struct Lies {
    pub positions: Option<Vec<i64>>,
    pub evaluation_commit: Option<Hash>,
    pub branch: Option<Hash>,
    pub policy_version: Option<u64>,
    pub decision_signer: Option<Keypair>,
    pub entropy: Option<Hash>,
    pub attested_entropy: Option<Hash>,
    pub tip: Option<Hash>,
    pub successor_sequence: Option<u64>,
    pub successor_policy: Option<Hash>,
    pub successor_catalog: Option<Hash>,
    /// Replace the fork the step records. The decision commits to whatever is here.
    pub fork: Option<Vec<CandidateRecord>>,
    pub chosen: Option<u64>,
}

pub fn no_lies() -> Lies {
    Lies {
        positions: None,
        evaluation_commit: None,
        branch: None,
        policy_version: None,
        decision_signer: None,
        entropy: None,
        attested_entropy: None,
        tip: None,
        successor_sequence: None,
        successor_policy: None,
        successor_catalog: None,
        fork: None,
        chosen: None,
    }
}

/// A fully signed step made with the device key, never passing through the enforcer.
pub fn forge(parent: &AttestedState, action: &str, lies: Lies) -> StepRecord {
    forge_with(&charter(), parent, propose(action), lies)
}

/// [`forge`], under any charter and for any proposal.
pub fn forge_with(
    charter: &SignedCharter,
    parent: &AttestedState,
    proposal: Proposal,
    lies: Lies,
) -> StepRecord {
    let device = &keys().device;
    let policy = charter.policy_hash();
    let catalog = charter.catalog_hash();
    let row = charter
        .catalog
        .row(&proposal.action)
        .expect("a catalog row")
        .clone();
    let branch = lies.branch.unwrap_or_else(|| {
        commit::branch_commitment(
            &parent.tip,
            &policy,
            &commit::proposal_commit(&proposal),
            Some(&row),
        )
    });
    let evaluation = evaluate(&charter.policy, &parent.positions, &row.measurement());
    let positions = lies
        .positions
        .unwrap_or_else(|| evaluation.successor.clone());
    let evaluation_commit = lies
        .evaluation_commit
        .unwrap_or_else(|| commit::evaluation_commit(&evaluation.canonical_bytes()));
    let (honest, _) = record_candidate(charter, &parent.tip, &parent.positions, &proposal);
    let fork = lies.fork.unwrap_or_else(|| vec![honest]);
    let chosen = lies.chosen.unwrap_or(0);
    let root = fork_root(&fork);
    let sequence = parent.sequence + 1;
    let successor = commit::successor_commit(sequence, &positions, &policy, &catalog);
    let decision_commit = commit::decision_commit(&commit::DecisionFields {
        parent_tip: &parent.tip,
        fork_root: &root,
        fork_size: fork.len() as u64,
        chosen,
        parent_state: &parent.commitment(),
        successor: &successor,
        policy: &policy,
        policy_version: lies.policy_version.unwrap_or(charter.policy.version),
        branch: &branch,
        evaluation: &evaluation_commit,
        evidence: &commit::evidence_commit(&catalog, &row),
    });
    let signer = lies.decision_signer.as_ref().unwrap_or(device);
    let decision_signature =
        dsm::crypto::sphincs::sphincs_sign(&signer.secret, &decision_commit).expect("sign");
    let operation = commit::decision_operation(
        &decision_commit,
        &device.public,
        &policy,
        &successor,
        &decision_signature,
    );
    let entropy = lies
        .entropy
        .unwrap_or_else(|| derive_transition_entropy(parent, &operation));
    let (devid, _, rel_key) = identity_of(&device.public);
    let tip = lies.tip.unwrap_or_else(|| {
        relationship_chain_tip_v2(
            &rel_key,
            &parent.tip,
            &devid,
            &operation.to_bytes(),
            &entropy,
            None,
        )
    });
    let next = AttestedState::attest(
        lies.successor_sequence.unwrap_or(sequence),
        tip,
        positions,
        lies.successor_policy.unwrap_or(policy),
        lies.successor_catalog.unwrap_or(catalog),
        lies.attested_entropy.unwrap_or(entropy),
        tip,
        device,
    )
    .expect("attest");
    StepRecord {
        parent_tip: parent.tip,
        proposal,
        row,
        branch_commitment: branch,
        evaluation_commit,
        fork,
        chosen,
        decision_commit,
        decision_signature,
        successor: next,
    }
}

/// The honest record of a candidate at a parent, as the enforcer would make it.
pub fn record(parent: &AttestedState, action: &str) -> CandidateRecord {
    record_candidate(&charter(), &parent.tip, &parent.positions, &propose(action)).0
}
