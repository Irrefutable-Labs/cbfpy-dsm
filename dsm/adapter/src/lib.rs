//! A separate experimental layer over DSM's public API. It changes nothing in DSM.
//!
//! The AI proposes what to do: a body act (look, move, drive, strike, speak, and the rest) or an
//! act from the signed catalog (replies, messages, files, commands). It never supplies a number or
//! a verdict. Every candidate branches from the authenticated parent and the whole fork is
//! committed. The protected-life rules in `ethical_world` decide, from the knowledge committed on
//! the chain, which are allowed, and the one chosen becomes a real step on a real
//! `dsm::core::state_machine::StateMachine`. What the senses see is committed as its own link. A
//! step verifies from its one parent and reads nothing behind it, as Amendment A14 has it for
//! acceptance; this policy layer adds a check beside acceptance and leaves acceptance itself
//! alone.
//!
//! The enforcer runs as its own service (`server`), holding the keys, the chain and the body. The
//! AI's side reaches it only through `protocol`.

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
pub mod model;
pub mod observe;
pub mod peer;
pub mod protocol;
pub mod report;
pub mod server;
pub mod session;
pub mod store;
pub mod update;
mod verify;

pub use charter::{keypair_from_seed, Catalog, CatalogRow, Keypair, Policy, SignedCharter, SYSTEM};
pub use config::{charter_config_text, parse_charter_config};
pub use enforcer::{
    audit_chain, check_considered, check_receipts, derive_transition_entropy, identity_of,
    pending_steps, steps_as_links, AdapterError, AttestedState, Audit, Decision, Enforcer, Future,
    FutureStatus, Link, Proposal, StepRecord, MAX_FORK,
};
pub use verify::{verify_genesis, verify_step, Refusal};

pub type Hash = [u8; 32];
