//! The chain on disk: one JSON line per entry, the genesis first, each step written and synced
//! before the enforcer installs it. Nothing read back is trusted: a store is audited in full and
//! replayed through core before an enforcer resumes from it.

use crate::enforcer::{AdapterError, AttestedState, StepRecord};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Entry {
    Genesis(AttestedState),
    Step(Box<StepRecord>),
}

pub struct ChainStore {
    dir: PathBuf,
}

fn io(e: std::io::Error) -> AdapterError {
    AdapterError::BadStore(e.to_string())
}

impl ChainStore {
    pub fn open(dir: &Path) -> Result<Self, AdapterError> {
        std::fs::create_dir_all(dir).map_err(io)?;
        Ok(ChainStore {
            dir: dir.to_path_buf(),
        })
    }

    fn chain(&self) -> PathBuf {
        self.dir.join("chain.jsonl")
    }

    fn device(&self) -> PathBuf {
        self.dir.join("device.pub")
    }

    pub fn exists(&self) -> bool {
        self.chain().exists()
    }

    fn write_line(file: &mut File, entry: &Entry) -> Result<(), AdapterError> {
        let mut line =
            serde_json::to_vec(entry).map_err(|e| AdapterError::BadStore(e.to_string()))?;
        line.push(b'\n');
        file.write_all(&line).map_err(io)?;
        file.sync_all().map_err(io)
    }

    /// Start a new chain. Refuses to overwrite one.
    pub fn begin(&self, genesis: &AttestedState, device_public: &[u8]) -> Result<(), AdapterError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.chain())
            .map_err(io)?;
        Self::write_line(&mut file, &Entry::Genesis(genesis.clone()))?;
        std::fs::write(self.device(), crate::hexser::to_hex(device_public)).map_err(io)
    }

    pub fn append(&self, step: &StepRecord) -> Result<(), AdapterError> {
        let mut file = OpenOptions::new()
            .append(true)
            .open(self.chain())
            .map_err(io)?;
        Self::write_line(&mut file, &Entry::Step(Box::new(step.clone())))
    }

    pub fn device_public(&self) -> Result<Vec<u8>, AdapterError> {
        let text = std::fs::read_to_string(self.device()).map_err(io)?;
        crate::hexser::decode(text.trim()).map_err(AdapterError::BadStore)
    }

    pub fn load(&self) -> Result<(AttestedState, Vec<StepRecord>), AdapterError> {
        let file = File::open(self.chain()).map_err(io)?;
        let mut genesis = None;
        let mut steps = Vec::new();
        for (n, line) in BufReader::new(file).lines().enumerate() {
            let line = line.map_err(io)?;
            let entry: Entry = serde_json::from_str(&line)
                .map_err(|e| AdapterError::BadStore(format!("line {}: {e}", n + 1)))?;
            match (entry, n) {
                (Entry::Genesis(g), 0) => genesis = Some(g),
                (Entry::Step(s), n) if n > 0 => steps.push(*s),
                _ => {
                    return Err(AdapterError::BadStore(format!(
                        "line {}: out of place",
                        n + 1
                    )))
                }
            }
        }
        let genesis = genesis.ok_or(AdapterError::BadStore("the store is empty".to_string()))?;
        Ok((genesis, steps))
    }
}
