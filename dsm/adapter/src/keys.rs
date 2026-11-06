//! Key files. A secret seed must be readable by its owner alone; a public key is plain hex.

use crate::enforcer::AdapterError;
use crate::hexser::decode;
use std::io::Read;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

fn config(e: impl std::fmt::Display) -> AdapterError {
    AdapterError::Config(e.to_string())
}

/// A 32-byte secret seed, refused if anyone but its owner can read it.
pub fn read_secret_seed(path: &Path) -> Result<[u8; 32], AdapterError> {
    let mode = std::fs::metadata(path)
        .map_err(|e| config(format!("{}: {e}", path.display())))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(config(format!(
            "{} is readable by others (mode {:o}); it must be owner-only",
            path.display(),
            mode & 0o777
        )));
    }
    let text =
        std::fs::read_to_string(path).map_err(|e| config(format!("{}: {e}", path.display())))?;
    decode(text.trim())
        .map_err(config)?
        .try_into()
        .map_err(|_| config("a seed must be 32 bytes of hexadecimal"))
}

/// Write a fresh random seed that only its owner can read. Refuses to replace one.
pub fn write_new_seed(path: &Path) -> Result<[u8; 32], AdapterError> {
    let mut seed = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut seed))
        .map_err(config)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(config)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| config(format!("{}: {e}", path.display())))?;
    std::io::Write::write_all(&mut file, crate::hexser::to_hex(&seed).as_bytes())
        .map_err(config)?;
    Ok(seed)
}

pub fn read_public_key(path: &Path) -> Result<Vec<u8>, AdapterError> {
    let text =
        std::fs::read_to_string(path).map_err(|e| config(format!("{}: {e}", path.display())))?;
    decode(text.trim()).map_err(config)
}
