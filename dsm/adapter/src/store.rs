//! The chain on disk: one JSON line per entry, the genesis and its charter first, each step,
//! observation or charter update written and synced before the enforcer installs it. Nothing read back is trusted: a store is audited in full and
//! replayed through core before an enforcer resumes from it.

use crate::act::EffectReceipt;
use crate::charter::SignedCharter;
use crate::enforcer::{AdapterError, AttestedState, Link, StepRecord};
use crate::update::UpdateRecord;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Entry {
    Genesis(Box<GenesisEntry>),
    Step(Box<StepRecord>),
    Update(Box<UpdateRecord>),
    Effect(Box<EffectReceipt>),
    Considered(Box<crate::fork::ConsideredFork>),
    Observed(Box<crate::observe::ObservedRecord>),
}

/// The first line: the genesis state, and the charter that governed it, in full.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenesisEntry {
    state: AttestedState,
    charter_text: String,
    #[serde(with = "crate::hexser::bytes")]
    charter_signature: Vec<u8>,
}

/// A stored chain as read back, not yet trusted.
pub struct Stored {
    pub genesis: AttestedState,
    /// The charter the chain began under. Every later charter is in an update link.
    pub genesis_charter: SignedCharter,
    /// Steps and charter updates, in order.
    pub links: Vec<Link>,
    pub receipts: Vec<EffectReceipt>,
    /// Forks put forward and superseded, each with the number of links stored before it.
    pub considered: Vec<(usize, crate::fork::ConsideredFork)>,
    /// A last line cut short by a stop mid-write. It was never installed: a step whose line
    /// is torn never committed, and a receipt whose line is torn leaves its act to be redone.
    pub torn_tail: bool,
}

pub struct ChainStore {
    dir: PathBuf,
}

fn io(e: std::io::Error) -> AdapterError {
    AdapterError::BadStore(e.to_string())
}

impl ChainStore {
    /// Open a store, making its directory readable by its owner alone.
    pub fn open(dir: &Path) -> Result<Self, AdapterError> {
        std::fs::create_dir_all(dir).map_err(io)?;
        std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .map_err(io)?;
        Ok(ChainStore {
            dir: dir.to_path_buf(),
        })
    }

    /// Open an existing store to read, changing nothing, not even its directory's mode.
    pub fn open_read_only(dir: &Path) -> Result<Self, AdapterError> {
        if !dir.is_dir() {
            return Err(AdapterError::BadStore(format!(
                "{} is not a store",
                dir.display()
            )));
        }
        Ok(ChainStore {
            dir: dir.to_path_buf(),
        })
    }

    /// Hold the store for this process alone. A second enforcer on the same store would commit
    /// beside the first, so it is refused. The lock lasts as long as the returned file.
    pub fn lock(&self) -> Result<File, AdapterError> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.dir.join("lock"))
            .map_err(io)?;
        // SAFETY: the descriptor is open for the life of `file`.
        let r = unsafe {
            libc::flock(
                std::os::unix::io::AsRawFd::as_raw_fd(&file),
                libc::LOCK_EX | libc::LOCK_NB,
            )
        };
        if r != 0 {
            return Err(AdapterError::BadStore(
                "another enforcer holds this store".to_string(),
            ));
        }
        Ok(file)
    }

    /// Cut a torn last line away, so the next line is not written onto it.
    pub fn drop_torn_tail(&self) -> Result<(), AdapterError> {
        let bytes = std::fs::read(self.chain()).map_err(io)?;
        if bytes.last() == Some(&b'\n') || bytes.is_empty() {
            return Ok(());
        }
        let keep = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        let file = OpenOptions::new()
            .write(true)
            .open(self.chain())
            .map_err(io)?;
        file.set_len(keep as u64).map_err(io)?;
        file.sync_all().map_err(io)
    }

    fn chain(&self) -> PathBuf {
        self.dir.join("chain.jsonl")
    }

    fn device(&self) -> PathBuf {
        self.dir.join("device.pub")
    }

    /// Whether a chain has begun here. A genesis line cut short by a stop means it has not.
    pub fn exists(&self) -> bool {
        std::fs::read(self.chain()).is_ok_and(|b| b.contains(&b'\n'))
    }

    fn write_line(file: &mut File, entry: &Entry) -> Result<(), AdapterError> {
        let mut line =
            serde_json::to_vec(entry).map_err(|e| AdapterError::BadStore(e.to_string()))?;
        line.push(b'\n');
        file.write_all(&line).map_err(io)?;
        file.sync_all().map_err(io)
    }

    /// Start a new chain where none has begun. A genesis line cut short by a stop is replaced;
    /// the caller has checked [`ChainStore::exists`] under the store's lock.
    pub fn begin(
        &self,
        genesis: &AttestedState,
        device_public: &[u8],
        charter: &SignedCharter,
    ) -> Result<(), AdapterError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(self.chain())
            .map_err(io)?;
        let entry = GenesisEntry {
            state: genesis.clone(),
            charter_text: crate::config::charter_config_text(&charter.policy, &charter.catalog),
            charter_signature: charter.signature.clone(),
        };
        Self::write_line(&mut file, &Entry::Genesis(Box::new(entry)))?;
        std::fs::write(self.device(), crate::hexser::to_hex(device_public)).map_err(io)
    }

    pub fn append(&self, step: &StepRecord) -> Result<(), AdapterError> {
        let mut file = OpenOptions::new()
            .append(true)
            .open(self.chain())
            .map_err(io)?;
        Self::write_line(&mut file, &Entry::Step(Box::new(step.clone())))
    }

    pub fn append_update(&self, update: &UpdateRecord) -> Result<(), AdapterError> {
        let mut file = OpenOptions::new()
            .append(true)
            .open(self.chain())
            .map_err(io)?;
        Self::write_line(&mut file, &Entry::Update(Box::new(update.clone())))
    }

    pub fn append_receipt(&self, receipt: &EffectReceipt) -> Result<(), AdapterError> {
        let mut file = OpenOptions::new()
            .append(true)
            .open(self.chain())
            .map_err(io)?;
        Self::write_line(&mut file, &Entry::Effect(Box::new(receipt.clone())))
    }

    pub fn append_observed(
        &self,
        observed: &crate::observe::ObservedRecord,
    ) -> Result<(), AdapterError> {
        let mut file = OpenOptions::new()
            .append(true)
            .open(self.chain())
            .map_err(io)?;
        Self::write_line(&mut file, &Entry::Observed(Box::new(observed.clone())))
    }

    pub fn append_considered(
        &self,
        fork: &crate::fork::ConsideredFork,
    ) -> Result<(), AdapterError> {
        let mut file = OpenOptions::new()
            .append(true)
            .open(self.chain())
            .map_err(io)?;
        Self::write_line(&mut file, &Entry::Considered(Box::new(fork.clone())))
    }

    pub fn device_public(&self) -> Result<Vec<u8>, AdapterError> {
        let text = std::fs::read_to_string(self.device()).map_err(io)?;
        crate::hexser::decode(text.trim()).map_err(AdapterError::BadStore)
    }

    pub fn load(&self) -> Result<Stored, AdapterError> {
        let bytes = std::fs::read(self.chain()).map_err(io)?;
        let text = String::from_utf8_lossy(&bytes);
        let mut lines: Vec<&str> = text.split('\n').collect();
        // Everything after the last newline is a line that never finished.
        let tail = lines.pop().unwrap_or("");
        let torn_tail = !tail.is_empty();
        let mut genesis = None;
        let mut links: Vec<Link> = Vec::new();
        let mut receipts: Vec<EffectReceipt> = Vec::new();
        let mut considered = Vec::new();
        for (n, line) in lines.iter().enumerate() {
            let entry: Entry = serde_json::from_str(line)
                .map_err(|e| AdapterError::BadStore(format!("line {}: {e}", n + 1)))?;
            match (entry, n) {
                (Entry::Genesis(g), 0) => genesis = Some(*g),
                (Entry::Step(s), n) if n > 0 => links.push(Link::Step(s)),
                (Entry::Update(u), n) if n > 0 => links.push(Link::Update(u)),
                (Entry::Observed(o), n) if n > 0 => links.push(Link::Observed(o)),
                // A receipt comes after its own step, and receipts come in order.
                (Entry::Effect(r), n)
                    if n > 0
                        && receipts
                            .last()
                            .is_none_or(|last| r.sequence > last.sequence)
                        && links.iter().any(|l| {
                            l.as_step()
                                .is_some_and(|s| s.successor.sequence == r.sequence)
                        }) =>
                {
                    receipts.push(*r)
                }
                (Entry::Considered(c), n) if n > 0 => considered.push((links.len(), *c)),
                _ => {
                    return Err(AdapterError::BadStore(format!(
                        "line {}: out of place",
                        n + 1
                    )))
                }
            }
        }
        let g = genesis.ok_or(AdapterError::BadStore("the store is empty".to_string()))?;
        let genesis_charter = crate::update::read_charter(&g.charter_text, &g.charter_signature)
            .ok_or_else(|| {
                AdapterError::BadStore("the genesis charter cannot be read".to_string())
            })?;
        Ok(Stored {
            genesis: g.state,
            genesis_charter,
            links,
            receipts,
            considered,
            torn_tail,
        })
    }
}
