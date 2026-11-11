//! The only way the AI's side reaches the enforcer: one JSON object per line over a Unix socket.

use crate::enforcer::Proposal;
use crate::report::FutureReport;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::{BufRead, Read, Write};

/// No message is longer than this. A longer one closes the connection.
pub const MAX_LINE: usize = 1 << 20;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// The catalog's actions by name and description, the fact register, and whether a body is
    /// attached. No numbers.
    Menu,
    Status,
    /// Branch these proposals from the current parent.
    Propose {
        proposals: Vec<Proposal>,
    },
    /// Commit one candidate of the open fork.
    Choose {
        fork_root: String,
        index: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuItem {
    pub action: String,
    pub description: String,
    /// Whether choosing it needs a text: what it will say or write.
    pub needs_text: bool,
    /// What it does and whom it affects, in the terms a declaration must use.
    pub effects: Vec<String>,
    pub affected: Vec<String>,
    /// The material consequences a declaration of it must disclose.
    pub material: Vec<String>,
    /// Facts from the register its words must tell people, by id.
    pub must_assert: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Menu {
        actions: Vec<MenuItem>,
        /// The register of verified facts the words are held to, as the charter signs it.
        facts: Vec<crate::honesty::Fact>,
        /// Whether a facility is attached, so system acts can be carried out.
        has_body: bool,
        /// The temperatures each protected species can bear, as the charter's foundation has them:
        /// species, lowest and highest °C.
        safe_temperatures: Vec<(String, i32, i32)>,
    },
    Status {
        sequence: u64,
        tip: String,
        dsm_root: String,
        policy_hash: String,
        /// What the chain knows: the map as seen, who was seen where, the body, the senses.
        knowledge: ethical_world::knowledge::Knowledge,
    },
    Forked {
        fork_root: String,
        futures: Vec<FutureReport>,
    },
    /// Committed: the enforcer has verified the step from its parent and stored it.
    Committed {
        sequence: u64,
        action: String,
        fork_size: usize,
        tip: String,
        decision: String,
        /// What the act wrote in the world, by path.
        effects: Vec<String>,
        /// For a look: what the senses reported, now committed.
        observed: Option<ethical_world::Observation>,
    },
    Refused {
        reason: String,
    },
    Error {
        message: String,
    },
}

pub fn send<T: Serialize>(w: &mut impl Write, value: &T) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    line.push(b'\n');
    w.write_all(&line)?;
    w.flush()
}

/// `None` at a clean end of stream.
pub fn receive<T: DeserializeOwned>(r: &mut impl BufRead) -> std::io::Result<Option<T>> {
    let mut line = Vec::new();
    let read = r
        .by_ref()
        .take(MAX_LINE as u64 + 1)
        .read_until(b'\n', &mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if line.len() > MAX_LINE {
        return Err(std::io::Error::other("message too long"));
    }
    serde_json::from_slice(&line)
        .map(Some)
        .map_err(std::io::Error::other)
}
