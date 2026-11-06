//! A separate experimental layer over DSM's public API. It changes nothing in DSM.
//!
//! The AI proposes actions. It never supplies a number: each action's measurement comes from a
//! catalog the policy authority signed. Candidate futures branch from the authenticated parent,
//! the engine decides which are eligible, and the one committed becomes a real step on a real
//! `dsm::core::state_machine::StateMachine`. A step verifies from its one parent and reads
//! nothing behind it, as Amendment A14 has it for acceptance; this policy layer adds a check
//! beside acceptance and leaves acceptance itself alone.

mod charter;
pub mod commit;
mod config;
mod enforcer;
mod verify;

pub use charter::{keypair_from_seed, Catalog, CatalogRow, Keypair, SignedCharter};
pub use config::parse_charter_config;
pub use enforcer::{
    derive_transition_entropy, identity_of, AdapterError, AttestedState, Decision, Enforcer, Future,
    FutureStatus, Proposal, StepRecord,
};
pub use verify::{verify_step, Refusal};

pub type Hash = [u8; 32];
