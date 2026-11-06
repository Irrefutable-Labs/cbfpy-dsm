//! Holds the device key and the real DSM head. The AI holds neither.

use crate::charter::{CatalogRow, Keypair, SignedCharter};
use crate::commit;
use crate::fork::{fork_root, record_candidate, CandidateRecord, Outcome};
use crate::verify::verify_step;
use crate::Hash;
use dsm::common::domain_tags::{TAG_DEVICE_ID, TAG_DSM_GENESIS_ENTROPY, TAG_DSM_STATE_ENTROPY};
use dsm::core::bilateral_transaction_manager::compute_smt_key;
use dsm::core::state_machine::StateMachine;
use dsm::crypto::blake3::{domain_hash_bytes, dsm_domain_hasher};
use dsm::crypto::sphincs;
use dsm::types::device_state::DeviceState;
use dsm::types::operations::Operation;
use ethical_band_engine::{evaluate, Evaluation, Measurement};
use serde::{Deserialize, Serialize};

const MAX_ACTION_BYTES: usize = 128;
const MAX_RATIONALE_BYTES: usize = 4_096;
/// No fork may be wider than this. It keeps a step's record bounded.
pub const MAX_FORK: usize = 16;

#[derive(Debug)]
pub enum AdapterError {
    BadCharter,
    BadStart,
    NotEligible,
    StaleFuture,
    /// A stored chain that does not verify, or does not replay to the same DSM head.
    BadStore(String),
    Dsm(String),
    Config(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for AdapterError {}

/// All the AI gets to say. There is no field for a score, and any extra field is refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub action: String,
    pub rationale: String,
}

impl Proposal {
    pub fn parse_model_output(text: &str) -> Result<Self, AdapterError> {
        let p: Proposal =
            serde_json::from_str(text.trim()).map_err(|e| AdapterError::Config(e.to_string()))?;
        p.check()?;
        Ok(p)
    }

    pub fn check(&self) -> Result<(), AdapterError> {
        if self.action.is_empty()
            || self.action.len() > MAX_ACTION_BYTES
            || self.rationale.len() > MAX_RATIONALE_BYTES
        {
            return Err(AdapterError::Config("proposal out of bounds".to_string()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FutureStatus {
    Eligible(Evaluation),
    Rejected(Evaluation),
    /// Not in the signed catalog, so there is nothing to evaluate.
    Unmeasured,
}

/// One pre-commit branch from the parent, carrying the whole fork it belongs to. It is a claim
/// until committed: `commit` works every branch out again and believes nothing written here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Future {
    pub index: usize,
    pub proposal: Proposal,
    pub parent_tip: Hash,
    pub branch_commitment: Hash,
    pub status: FutureStatus,
    pub fork_root: Hash,
    pub fork: Vec<CandidateRecord>,
}

/// A position vector, its place on the chain, and what governs it, under the device key. The two
/// entropy fields are what core's next transition reads from this state: `e_n` and `h_n`, with
/// the genesis derivation before the first step. They let a verifier derive a step's entropy from
/// this one state instead of being told it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttestedState {
    pub sequence: u64,
    #[serde(with = "crate::hexser::array")]
    pub tip: Hash,
    pub positions: Vec<i64>,
    #[serde(with = "crate::hexser::array")]
    pub policy_hash: Hash,
    #[serde(with = "crate::hexser::array")]
    pub catalog_hash: Hash,
    #[serde(with = "crate::hexser::array")]
    pub entropy: Hash,
    #[serde(with = "crate::hexser::array")]
    pub entropy_hash: Hash,
    #[serde(with = "crate::hexser::bytes")]
    pub signature: Vec<u8>,
}

impl AttestedState {
    #[allow(clippy::too_many_arguments)]
    pub fn attest(
        sequence: u64,
        tip: Hash,
        positions: Vec<i64>,
        policy_hash: Hash,
        catalog_hash: Hash,
        entropy: Hash,
        entropy_hash: Hash,
        device: &Keypair,
    ) -> Result<Self, AdapterError> {
        let mut state = AttestedState {
            sequence,
            tip,
            positions,
            policy_hash,
            catalog_hash,
            entropy,
            entropy_hash,
            signature: vec![],
        };
        state.signature = sphincs::sphincs_sign(&device.secret, &state.commitment())
            .map_err(|e| AdapterError::Dsm(e.to_string()))?;
        Ok(state)
    }

    pub fn commitment(&self) -> Hash {
        commit::state_commit(&commit::StateFields {
            sequence: self.sequence,
            tip: &self.tip,
            positions: &self.positions,
            policy: &self.policy_hash,
            catalog: &self.catalog_hash,
            entropy: &self.entropy,
            entropy_hash: &self.entropy_hash,
        })
    }
}

/// `e_n` before a relationship's first step, from the genesis root, as core has it.
pub(crate) fn genesis_entropy(root: &Hash) -> Hash {
    let mut h = dsm_domain_hasher(TAG_DSM_GENESIS_ENTROPY);
    h.update(root);
    *h.finalize().as_bytes()
}

/// `e_{n+1} = H(e_n || op || h_n)`, read from one attested state. `commit` checks this against
/// what core itself derived for every real step, so the two cannot drift apart unnoticed.
pub fn derive_transition_entropy(parent: &AttestedState, operation: &Operation) -> Hash {
    let mut h = dsm_domain_hasher(TAG_DSM_STATE_ENTROPY);
    h.update(&parent.entropy);
    h.update(&operation.to_bytes());
    h.update(&parent.entropy_hash);
    *h.finalize().as_bytes()
}

/// Everything a verifier needs beyond the parent, the charter and the keys. The chosen
/// candidate's fields are repeated at the top for reading. The verifier holds them to `fork`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepRecord {
    #[serde(with = "crate::hexser::array")]
    pub parent_tip: Hash,
    pub proposal: Proposal,
    pub row: CatalogRow,
    #[serde(with = "crate::hexser::array")]
    pub branch_commitment: Hash,
    #[serde(with = "crate::hexser::array")]
    pub evaluation_commit: Hash,
    /// Every candidate put forward at the parent, in the order proposed.
    pub fork: Vec<CandidateRecord>,
    pub chosen: u64,
    #[serde(with = "crate::hexser::array")]
    pub decision_commit: Hash,
    #[serde(with = "crate::hexser::bytes")]
    pub decision_signature: Vec<u8>,
    pub successor: AttestedState,
}

#[derive(Debug)]
pub enum Decision {
    Committed {
        step: Box<StepRecord>,
    },
    /// Nothing proposed may be committed. Nothing was committed. Ask for an alternative.
    NoEligibleFuture {
        futures: Vec<Future>,
    },
    /// The chooser declined every eligible future. Nothing was committed.
    Declined {
        futures: Vec<Future>,
    },
}

/// The device's identity and its self-relationship, the same derivation core's tests use.
pub fn identity_of(device_public: &[u8]) -> (Hash, Hash, Hash) {
    let devid = domain_hash_bytes(TAG_DEVICE_ID, device_public);
    let genesis = domain_hash_bytes(TAG_DEVICE_ID, &devid);
    let rel_key = compute_smt_key(&devid, &devid);
    (devid, genesis, rel_key)
}

pub struct Enforcer {
    machine: StateMachine,
    device: Keypair,
    devid: Hash,
    rel_key: Hash,
    charter: SignedCharter,
    state: AttestedState,
}

impl Enforcer {
    /// Begin a chain under a charter, with every band at its center. The start is not a choice,
    /// so nothing can begin a chain already past a threshold. The policy cannot change after this
    /// either: a step is checked against the policy its parent names, and this prototype has no
    /// rotation path.
    pub fn genesis(
        device: Keypair,
        charter: SignedCharter,
        trusted_authority: &[u8],
    ) -> Result<Self, AdapterError> {
        charter.verify(trusted_authority)?;
        let start: Vec<i64> = charter.policy.bands.iter().map(|b| b.center).collect();
        let resting = evaluate(
            &charter.policy,
            &start,
            &Measurement {
                displacement: vec![0; start.len()],
                tags: vec![],
            },
        );
        if !resting.is_eligible() {
            return Err(AdapterError::BadStart);
        }
        let (devid, genesis, rel_key) = identity_of(&device.public);
        let head = DeviceState::genesis_head(genesis, devid, device.public.clone());
        let tip = head
            .chain_tip(&rel_key)
            .ok_or_else(|| AdapterError::Dsm("no self-relationship at genesis".to_string()))?;
        let root = head.root();
        let mut machine = StateMachine::new();
        machine.set_device_head(head);
        let state = AttestedState::attest(
            0,
            tip,
            start,
            charter.policy_hash(),
            charter.catalog_hash(),
            genesis_entropy(&root),
            root,
            &device,
        )?;
        Ok(Enforcer {
            machine,
            device,
            devid,
            rel_key,
            charter,
            state,
        })
    }

    /// Rebuild an enforcer from a stored chain: audited in full, then this enforcer's own DSM
    /// head is the one the audit's replay through core produced.
    pub fn resume(
        device: Keypair,
        charter: SignedCharter,
        trusted_authority: &[u8],
        genesis: &AttestedState,
        steps: &[StepRecord],
    ) -> Result<Self, AdapterError> {
        let mut e = Enforcer::genesis(device, charter, trusted_authority)?;
        if e.state != *genesis {
            return Err(AdapterError::BadStore(
                "the stored genesis is not this device's genesis under this charter".to_string(),
            ));
        }
        let audit = audit_chain(
            &e.device.public,
            &e.charter,
            trusted_authority,
            genesis,
            steps,
        )?;
        e.machine = audit.machine;
        e.state = audit.last;
        Ok(e)
    }

    pub fn tip(&self) -> Hash {
        self.state.tip
    }

    /// The real device root held by DSM's state machine.
    pub fn dsm_root(&self) -> Hash {
        self.machine.device_head().map_or([0; 32], |h| h.root())
    }

    pub fn positions(&self) -> &[i64] {
        &self.state.positions
    }

    pub fn state(&self) -> AttestedState {
        self.state.clone()
    }

    pub fn charter(&self) -> &SignedCharter {
        &self.charter
    }

    pub fn device_public(&self) -> &[u8] {
        &self.device.public
    }

    fn records_for(&self, proposals: &[Proposal]) -> Vec<(CandidateRecord, Option<Evaluation>)> {
        proposals
            .iter()
            .map(|p| record_candidate(&self.charter, &self.state.tip, &self.state.positions, p))
            .collect()
    }

    /// Branch every proposal from the current parent, and commit to the set. Nothing here touches
    /// the chain. Proposals past [`MAX_FORK`] are dropped, so a fork stays bounded.
    pub fn fork(&self, proposals: &[Proposal]) -> Vec<Future> {
        let proposals = &proposals[..proposals.len().min(MAX_FORK)];
        let worked = self.records_for(proposals);
        let records: Vec<CandidateRecord> = worked.iter().map(|(r, _)| r.clone()).collect();
        let root = fork_root(&records);
        worked
            .into_iter()
            .enumerate()
            .map(|(index, (record, evaluation))| Future {
                index,
                proposal: record.proposal.clone(),
                parent_tip: self.state.tip,
                branch_commitment: record.branch,
                status: match (record.outcome, evaluation) {
                    (Outcome::Eligible, Some(e)) => FutureStatus::Eligible(e),
                    (_, Some(e)) => FutureStatus::Rejected(e),
                    (_, None) => FutureStatus::Unmeasured,
                },
                fork_root: root,
                fork: records.clone(),
            })
            .collect()
    }

    /// Fork, show the chooser only what is eligible, and commit its pick: a position in what it
    /// was shown.
    pub fn decide(
        &mut self,
        proposals: &[Proposal],
        choose: impl FnOnce(&[&Future]) -> Option<usize>,
    ) -> Result<Decision, AdapterError> {
        let futures = self.fork(proposals);
        let eligible: Vec<&Future> = futures
            .iter()
            .filter(|f| matches!(f.status, FutureStatus::Eligible(_)))
            .collect();
        if eligible.is_empty() {
            return Ok(Decision::NoEligibleFuture { futures });
        }
        let Some(pick) = choose(&eligible) else {
            return Ok(Decision::Declined { futures });
        };
        let future = *eligible.get(pick).ok_or(AdapterError::NotEligible)?;
        let step = self.commit(future)?;
        Ok(Decision::Committed {
            step: Box::new(step),
        })
    }

    pub fn commit(&mut self, future: &Future) -> Result<StepRecord, AdapterError> {
        self.commit_with(future, |_| Ok(()))
    }

    /// Make a future the successor. Every branch of its fork is worked out again from the current
    /// parent, so a forged or stale `Future` carries nothing through. `persist` sees the finished
    /// step before anything is installed: if it fails, the chain does not move.
    pub fn commit_with(
        &mut self,
        future: &Future,
        persist: impl FnOnce(&StepRecord) -> Result<(), AdapterError>,
    ) -> Result<StepRecord, AdapterError> {
        if future.parent_tip != self.state.tip {
            return Err(AdapterError::StaleFuture);
        }
        let proposals: Vec<Proposal> = future.fork.iter().map(|r| r.proposal.clone()).collect();
        if proposals.is_empty() || proposals.len() > MAX_FORK {
            return Err(AdapterError::NotEligible);
        }
        let worked = self.records_for(&proposals);
        let records: Vec<CandidateRecord> = worked.iter().map(|(r, _)| r.clone()).collect();
        if records != future.fork || fork_root(&records) != future.fork_root {
            return Err(AdapterError::NotEligible);
        }
        let Some((chosen, Some(evaluation))) = worked.get(future.index).cloned() else {
            return Err(AdapterError::NotEligible);
        };
        if chosen.proposal != future.proposal
            || chosen.branch != future.branch_commitment
            || chosen.outcome != Outcome::Eligible
            || !evaluation.is_eligible()
        {
            return Err(AdapterError::NotEligible);
        }
        let row = chosen.row.clone().ok_or(AdapterError::NotEligible)?;

        let policy = self.state.policy_hash;
        let catalog = self.state.catalog_hash;
        let sequence = self.state.sequence + 1;
        let successor =
            commit::successor_commit(sequence, &evaluation.successor, &policy, &catalog);
        let evaluation_commit = commit::evaluation_commit(&evaluation.canonical_bytes());
        let decision_commit = commit::decision_commit(&commit::DecisionFields {
            parent_tip: &self.state.tip,
            fork_root: &future.fork_root,
            fork_size: records.len() as u64,
            chosen: future.index as u64,
            parent_state: &self.state.commitment(),
            successor: &successor,
            policy: &policy,
            policy_version: self.charter.policy.version,
            branch: &chosen.branch,
            evaluation: &evaluation_commit,
            evidence: &commit::evidence_commit(&catalog, &row),
        });
        let dsm_error = |e: dsm::types::error::DsmError| AdapterError::Dsm(e.to_string());
        let decision_signature =
            sphincs::sphincs_sign(&self.device.secret, &decision_commit).map_err(dsm_error)?;
        let operation = commit::decision_operation(
            &decision_commit,
            &self.device.public,
            &policy,
            &successor,
            &decision_signature,
        );

        // Core's own two phases. Nothing is installed until the outcome is checked, attested
        // and persisted.
        let derived = derive_transition_entropy(&self.state, &operation);
        let outcome = self
            .machine
            .prepare_advance_relationship(self.rel_key, self.devid, operation, &[], None)
            .map_err(dsm_error)?;
        // Core leaves the swap to its caller: the outcome must extend the head still held.
        if Some(outcome.parent_r_a) != self.machine.device_head().map(|h| h.root()) {
            return Err(AdapterError::StaleFuture);
        }
        let (consumed, produced) = outcome.relationship_pair();
        if consumed != self.state.tip {
            return Err(AdapterError::StaleFuture);
        }
        let entropy = outcome.transition_entropy();
        if entropy != derived {
            return Err(AdapterError::Dsm(
                "core derived a different entropy from the verifier's rule".to_string(),
            ));
        }
        let next = AttestedState::attest(
            sequence,
            produced,
            evaluation.successor.clone(),
            policy,
            catalog,
            entropy,
            produced,
            &self.device,
        )?;
        let step = StepRecord {
            parent_tip: self.state.tip,
            proposal: chosen.proposal.clone(),
            row,
            branch_commitment: chosen.branch,
            evaluation_commit,
            fork: records,
            chosen: future.index as u64,
            decision_commit,
            decision_signature,
            successor: next.clone(),
        };
        persist(&step)?;
        self.machine.commit_advance(&outcome);
        self.state = next;
        Ok(step)
    }
}

/// What an audit established about a stored chain.
pub struct Audit {
    pub last: AttestedState,
    pub dsm_root: Hash,
    /// For each step: how many candidates were considered, and how many of them were refused or
    /// unmeasured.
    pub forks: Vec<(usize, usize, usize)>,
    machine: StateMachine,
}

/// Check a whole stored chain with only public material: the genesis against core's own genesis
/// head, each step from its one parent, and each step replayed through core's transition so the
/// DSM head is rebuilt rather than read from the store.
pub fn audit_chain(
    device_public: &[u8],
    charter: &SignedCharter,
    trusted_authority: &[u8],
    genesis: &AttestedState,
    steps: &[StepRecord],
) -> Result<Audit, AdapterError> {
    charter.verify(trusted_authority)?;
    crate::verify::verify_genesis(genesis, charter, device_public)
        .map_err(|r| AdapterError::BadStore(format!("the genesis does not verify: {r:?}")))?;
    let (devid, genesis_digest, rel_key) = identity_of(device_public);
    let mut machine = StateMachine::new();
    machine.set_device_head(DeviceState::genesis_head(
        genesis_digest,
        devid,
        device_public.to_vec(),
    ));
    let mut state = genesis.clone();
    let mut forks = Vec::with_capacity(steps.len());
    for (n, step) in steps.iter().enumerate() {
        verify_step(&state, step, charter, trusted_authority, device_public).map_err(|r| {
            AdapterError::BadStore(format!("step {} does not verify: {r:?}", n + 1))
        })?;
        let operation = commit::decision_operation(
            &step.decision_commit,
            device_public,
            &state.policy_hash,
            &commit::successor_commit(
                step.successor.sequence,
                &step.successor.positions,
                &state.policy_hash,
                &state.catalog_hash,
            ),
            &step.decision_signature,
        );
        let outcome = machine
            .prepare_advance_relationship(rel_key, devid, operation, &[], None)
            .map_err(|err| AdapterError::Dsm(err.to_string()))?;
        if outcome.relationship_pair().1 != step.successor.tip {
            return Err(AdapterError::BadStore(format!(
                "step {} does not replay to its tip through core",
                n + 1
            )));
        }
        machine.commit_advance(&outcome);
        let refused = step
            .fork
            .iter()
            .filter(|r| r.outcome == Outcome::Refused)
            .count();
        let unmeasured = step
            .fork
            .iter()
            .filter(|r| r.outcome == Outcome::Unmeasured)
            .count();
        forks.push((step.fork.len(), refused, unmeasured));
        state = step.successor.clone();
    }
    let dsm_root = machine.device_head().map_or([0; 32], |h| h.root());
    Ok(Audit {
        last: state,
        dsm_root,
        forks,
        machine,
    })
}
