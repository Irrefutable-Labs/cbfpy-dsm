//! Holds the device key and the real DSM head. The AI holds neither.

use crate::charter::{CatalogRow, Keypair, SignedCharter};
use crate::commit;
use crate::Hash;
use dsm::common::domain_tags::{TAG_DEVICE_ID, TAG_DSM_GENESIS_ENTROPY, TAG_DSM_STATE_ENTROPY};
use dsm::core::bilateral_transaction_manager::compute_smt_key;
use dsm::core::state_machine::StateMachine;
use dsm::crypto::blake3::{domain_hash_bytes, dsm_domain_hasher};
use dsm::crypto::sphincs;
use dsm::types::device_state::DeviceState;
use dsm::types::operations::Operation;
use ethical_band_engine::{evaluate, Evaluation, Measurement};
use serde::Deserialize;

const MAX_ACTION_BYTES: usize = 128;
const MAX_RATIONALE_BYTES: usize = 4_096;

#[derive(Debug)]
pub enum AdapterError {
    BadCharter,
    BadStart,
    NotEligible,
    StaleFuture,
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
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub action: String,
    pub rationale: String,
}

impl Proposal {
    pub fn parse_model_output(text: &str) -> Result<Self, AdapterError> {
        let p: Proposal =
            serde_json::from_str(text.trim()).map_err(|e| AdapterError::Config(e.to_string()))?;
        if p.action.is_empty()
            || p.action.len() > MAX_ACTION_BYTES
            || p.rationale.len() > MAX_RATIONALE_BYTES
        {
            return Err(AdapterError::Config("proposal out of bounds".to_string()));
        }
        Ok(p)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FutureStatus {
    Eligible(Evaluation),
    Rejected(Evaluation),
    /// Not in the signed catalog, so there is nothing to evaluate.
    Unmeasured,
}

/// One pre-commit branch from the parent. It is a claim until committed: `commit` works the
/// answer out again and believes nothing written here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Future {
    pub index: usize,
    pub proposal: Proposal,
    pub parent_tip: Hash,
    pub branch_commitment: Hash,
    pub status: FutureStatus,
}

/// A position vector, its place on the chain, and what governs it, under the device key. The two
/// entropy fields are what core's next transition reads from this state: `e_n` and `h_n`, with
/// the genesis derivation before the first step. They let a verifier derive a step's entropy from
/// this one state instead of being told it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttestedState {
    pub sequence: u64,
    pub tip: Hash,
    pub positions: Vec<i64>,
    pub policy_hash: Hash,
    pub catalog_hash: Hash,
    pub entropy: Hash,
    pub entropy_hash: Hash,
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

/// Everything a verifier needs beyond the parent, the charter and the keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepRecord {
    pub parent_tip: Hash,
    pub proposal: Proposal,
    pub row: CatalogRow,
    pub branch_commitment: Hash,
    pub evaluation_commit: Hash,
    pub decision_commit: Hash,
    pub decision_signature: Vec<u8>,
    pub successor: AttestedState,
}

#[derive(Debug)]
pub enum Decision {
    Committed { step: Box<StepRecord> },
    /// Nothing proposed may be committed. Nothing was committed. Ask for an alternative.
    NoEligibleFuture { futures: Vec<Future> },
    /// The chooser declined every eligible future. Nothing was committed.
    Declined { futures: Vec<Future> },
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

    fn branch_of(&self, proposal: &Proposal, row: Option<&CatalogRow>) -> Hash {
        commit::branch_commitment(
            &self.state.tip,
            &self.state.policy_hash,
            &commit::proposal_commit(&proposal.action, &proposal.rationale),
            row,
        )
    }

    /// Branch every proposal from the current parent. Nothing here touches the chain.
    pub fn fork(&self, proposals: &[Proposal]) -> Vec<Future> {
        proposals
            .iter()
            .enumerate()
            .map(|(index, proposal)| {
                let row = self.charter.catalog.row(&proposal.action);
                let status = match row {
                    None => FutureStatus::Unmeasured,
                    Some(r) => {
                        let e =
                            evaluate(&self.charter.policy, &self.state.positions, &r.measurement());
                        if e.is_eligible() {
                            FutureStatus::Eligible(e)
                        } else {
                            FutureStatus::Rejected(e)
                        }
                    }
                };
                Future {
                    index,
                    proposal: proposal.clone(),
                    parent_tip: self.state.tip,
                    branch_commitment: self.branch_of(proposal, row),
                    status,
                }
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

    /// Make a future the successor. Everything is worked out again from the current parent, so
    /// a forged or stale `Future` cannot carry anything through.
    pub fn commit(&mut self, future: &Future) -> Result<StepRecord, AdapterError> {
        if future.parent_tip != self.state.tip {
            return Err(AdapterError::StaleFuture);
        }
        let row = self
            .charter
            .catalog
            .row(&future.proposal.action)
            .ok_or(AdapterError::NotEligible)?
            .clone();
        if future.branch_commitment != self.branch_of(&future.proposal, Some(&row)) {
            return Err(AdapterError::NotEligible);
        }
        let evaluation = evaluate(
            &self.charter.policy,
            &self.state.positions,
            &row.measurement(),
        );
        if !evaluation.is_eligible() {
            return Err(AdapterError::NotEligible);
        }

        let policy = self.state.policy_hash;
        let catalog = self.state.catalog_hash;
        let sequence = self.state.sequence + 1;
        let successor = commit::successor_commit(sequence, &evaluation.successor, &policy, &catalog);
        let evaluation_commit = commit::evaluation_commit(&evaluation.canonical_bytes());
        let decision_commit = commit::decision_commit(&commit::DecisionFields {
            parent_tip: &self.state.tip,
            parent_state: &self.state.commitment(),
            successor: &successor,
            policy: &policy,
            policy_version: self.charter.policy.version,
            branch: &future.branch_commitment,
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

        // Core's own two phases. Nothing is installed until the outcome is checked and attested.
        let operation_copy = operation.clone();
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
        if entropy != derive_transition_entropy(&self.state, &operation_copy) {
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
            proposal: future.proposal.clone(),
            row,
            branch_commitment: future.branch_commitment,
            evaluation_commit,
            decision_commit,
            decision_signature,
            successor: next.clone(),
        };
        self.machine.commit_advance(&outcome);
        self.state = next;
        Ok(step)
    }
}
