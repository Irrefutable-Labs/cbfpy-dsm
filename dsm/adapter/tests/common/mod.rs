//! Shared setup: real SPHINCS+ keys from fixed test seeds, a real signed charter, a real
//! `StateMachine` head inside the enforcer. Nothing here stands in for production behavior.
#![allow(dead_code)]

use dsm::types::device_state::relationship_chain_tip_v2;
use ethical_autonomy_adapter::commit;
use ethical_autonomy_adapter::fork::{
    evaluate, fork_root, record_candidate, what_it_does, CandidateRecord,
};
use ethical_autonomy_adapter::{
    derive_transition_entropy, identity_of, keypair_from_seed, AttestedState, Catalog, CatalogRow,
    Enforcer, Hash, Keypair, Policy, Proposal, SignedCharter, StepRecord,
};
use ethical_world::knowledge::{key, Knowledge};
use ethical_world::sim::World;
use ethical_world::{
    Act, ActKind, Cell, Foundation, Kind, KnownBeing, Operational, Species, Terrain,
};
use std::collections::BTreeMap;
use std::sync::OnceLock;

fn species(name: &str, mass_mg: u64, length_mm: u32, kind: Kind) -> Species {
    Species {
        name: name.to_string(),
        adult_mass_mg: mass_mg,
        length_mm,
        kind,
    }
}

pub fn foundation() -> Foundation {
    Foundation {
        threshold_mg: 1_500,
        always_protected: vec!["honeybee".to_string()],
        species: vec![
            species("human", 62_000_000, 1_700, Kind::Human),
            species("deer", 70_000_000, 1_600, Kind::Wild),
            species("rat", 250_000, 230, Kind::Wild),
            species("honeybee", 100, 12, Kind::Wild),
            species("ant", 3, 5, Kind::Wild),
        ],
        prohibited: vec![ActKind::AimWeapon, ActKind::FireWeapon],
    }
}

pub fn operational() -> Operational {
    Operational {
        fresh_for: 8,
        senses_fresh_for: 6,
        checks_for_injury: 1,
        checks_for_lethal: 2,
        braking_cm_s2: 500,
        top_speed_cm_s: 1_500,
    }
}

pub fn policy() -> Policy {
    Policy {
        version: 1,
        foundation: foundation(),
        operational: operational(),
    }
}

/// A catalog act with no effect outside the chain.
pub fn row(action: &str) -> CatalogRow {
    CatalogRow {
        action: action.to_string(),
        description: format!("{action} (test row)"),
        act: ethical_autonomy_adapter::act::Act::None,
        material: vec![],
        must_assert: vec![],
    }
}

pub fn catalog() -> Catalog {
    Catalog {
        rows: vec![
            row("hold"),
            row("explore_a_little"),
            row("pull_back"),
            row("add_precautions"),
        ],
        ..Catalog::default()
    }
}

/// A small simulated world: a corridor of open cells from (0, 0) to (6, 0), with whoever is given.
pub fn corridor(beings: Vec<KnownBeing>) -> World {
    let mut terrain = BTreeMap::new();
    for x in 0..=6 {
        terrain.insert(key((x, 0)), Terrain::Open);
    }
    World {
        terrain,
        beings,
        body: (0, 0),
        resolution_mm: 5,
        sight: 12,
        sensor_covered: false,
        events: vec![],
        tick: 0,
        harms: vec![],
    }
}

pub fn being(id: &str, species: &str, cell: Cell) -> KnownBeing {
    KnownBeing {
        id: id.to_string(),
        species: species.to_string(),
        cell,
        levels: [3; 6],
    }
}

/// A body act as the AI proposes it, declared.
pub fn body(act: Act) -> Proposal {
    let declared = !matches!(act, Act::Observe { .. } | Act::Wait {});
    Proposal {
        action: ethical_autonomy_adapter::BODY.to_string(),
        rationale: "because".to_string(),
        text: String::new(),
        declaration: declared.then(|| ethical_autonomy_adapter::disclosure::Declaration {
            purpose: "to do it".to_string(),
            information: vec!["situation".to_string()],
            affected: vec![],
            effects: vec![],
            consequences: vec![],
            assertions: vec![],
        }),
        body: Some(act),
    }
}

/// Commit a look through the enforcer, and what the simulated world showed, as the service does.
pub fn look(e: &mut Enforcer, world: &mut World, cells: &[Cell]) {
    let futures = e.fork(&[body(Act::Observe {
        cells: cells.to_vec(),
    })]);
    e.commit(&futures[0]).expect("a look commits");
    let obs = world.observe(cells);
    e.record_observation(obs, |_| Ok(()))
        .expect("the observation commits");
}

/// Commit a body act through the enforcer and carry it out in the simulated world.
pub fn act(e: &mut Enforcer, world: &mut World, act: Act) -> StepRecord {
    let futures = e.fork(&[body(act.clone())]);
    let step = e.commit(&futures[0]).expect("the act commits");
    world.perform(&act);
    step
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
    Enforcer::genesis(
        keys().device.clone(),
        charter(),
        &keys().authority.public,
        (0, 0),
    )
    .expect("genesis")
}

pub fn propose(action: &str) -> Proposal {
    Proposal {
        action: action.to_string(),
        rationale: format!("I propose {action}"),
        text: String::new(),
        declaration: None,
        body: None,
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
    pub knowledge: Option<Knowledge>,
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
        knowledge: None,
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
    let row = if proposal.body.is_some() {
        None
    } else {
        Some(
            charter
                .catalog
                .row(&proposal.action)
                .expect("a catalog row")
                .clone(),
        )
    };
    let what = what_it_does(row.as_ref(), &proposal);
    let branch = lies.branch.unwrap_or_else(|| {
        commit::branch_commitment(
            &parent.tip,
            &policy,
            &commit::proposal_commit(&proposal),
            &what,
        )
    });
    let evaluation = evaluate(charter, &parent.knowledge, &proposal);
    let knowledge = lies
        .knowledge
        .unwrap_or_else(|| evaluation.successor.clone());
    let evaluation_commit = lies
        .evaluation_commit
        .unwrap_or_else(|| commit::evaluation_commit(&evaluation.canonical_bytes()));
    let (honest, _) = record_candidate(charter, &parent.tip, &parent.knowledge, &proposal);
    let fork = lies.fork.unwrap_or_else(|| vec![honest]);
    let chosen = lies.chosen.unwrap_or(0);
    let root = fork_root(&fork);
    let sequence = parent.sequence + 1;
    let successor = commit::successor_commit(
        sequence,
        &commit::knowledge_hash(&knowledge),
        &policy,
        &catalog,
    );
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
        evidence: &commit::evidence_commit(&catalog, &what),
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
        knowledge,
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
    record_candidate(&charter(), &parent.tip, &parent.knowledge, &propose(action)).0
}

/// The honest record of any proposal at a parent, as the enforcer would make it.
pub fn record_of(parent: &AttestedState, proposal: Proposal) -> CandidateRecord {
    record_candidate(&charter(), &parent.tip, &parent.knowledge, &proposal).0
}

/// A body act the enforcer refuses at genesis: it has not looked, so its body can do nothing.
pub fn blind_step() -> Proposal {
    body(Act::Move { path: vec![(1, 0)] })
}
