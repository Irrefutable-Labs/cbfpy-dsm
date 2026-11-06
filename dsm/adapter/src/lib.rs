//! A separate experimental layer over DSM's public API. It changes nothing in DSM.
//!
//! The AI proposes actions. It never supplies a number: each action's measurement comes from a
//! catalog the policy authority signed. Every candidate branches from the authenticated parent
//! and the whole fork is committed. The engine decides which are eligible, and the one chosen
//! becomes a real step on a real `dsm::core::state_machine::StateMachine`. A step verifies from
//! its one parent and reads nothing behind it, as Amendment A14 has it for acceptance; this
//! policy layer adds a check beside acceptance and leaves acceptance itself alone.
//!
//! The enforcer runs as its own service (`server`), holding the keys and the chain. The AI's side
//! reaches it only through `protocol`.

pub mod act;
mod charter;
pub mod commit;
pub(crate) mod config;
pub mod console;
pub mod disclosure;
mod enforcer;
pub mod fork;
pub mod hexser;
pub mod honesty;
pub mod keys;
pub mod peer;
pub mod protocol;
pub mod report;
pub mod server;
pub mod store;
pub mod update;
mod verify;

pub use charter::{keypair_from_seed, Catalog, CatalogRow, Keypair, SignedCharter};
pub use config::{charter_config_text, parse_charter_config};
pub use enforcer::{
    audit_chain, check_considered, check_receipts, derive_transition_entropy, identity_of,
    pending_steps, steps_as_links, AdapterError, AttestedState, Audit, Decision, Enforcer, Future,
    FutureStatus, Link, Proposal, StepRecord, MAX_FORK,
};
pub use verify::{verify_genesis, verify_step, Refusal};

pub type Hash = [u8; 32];
