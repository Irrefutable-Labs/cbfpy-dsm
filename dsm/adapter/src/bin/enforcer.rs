//! The enforcer service. It holds the device key, the chain and the DSM head, and listens on a
//! Unix socket that only its own user can open. A model never runs in this process.
//!
//! ethical-enforcer --store DIR --charter C --signature SIG --device-seed FILE --socket PATH
//!
//! The device seed file must be readable by its owner alone. A store that exists is audited in
//! full and replayed through core before anything is served. One that does not verify stops it.

use ethical_autonomy_adapter::hexser::{decode, to_hex};
use ethical_autonomy_adapter::server::Service;
use ethical_autonomy_adapter::store::ChainStore;
use ethical_autonomy_adapter::{
    keypair_from_seed, parse_charter_config, Enforcer, SignedCharter, TRUSTED_AUTHORITY_PUBLIC,
};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::ExitCode;

struct Args {
    store: PathBuf,
    charter: PathBuf,
    signature: PathBuf,
    device_seed: PathBuf,
    socket: PathBuf,
}

fn args() -> Result<Args, String> {
    let mut a = std::env::args().skip(1);
    let (mut store, mut charter, mut signature, mut seed, mut socket) =
        (None, None, None, None, None);
    while let Some(flag) = a.next() {
        let value = a
            .next()
            .map(PathBuf::from)
            .ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--store" => store = Some(value),
            "--charter" => charter = Some(value),
            "--signature" => signature = Some(value),
            "--device-seed" => seed = Some(value),
            "--socket" => socket = Some(value),
            other => return Err(format!("unknown flag {other}")),
        }
    }
    Ok(Args {
        store: store.ok_or("--store is required")?,
        charter: charter.ok_or("--charter is required")?,
        signature: signature.ok_or("--signature is required")?,
        device_seed: seed.ok_or("--device-seed is required")?,
        socket: socket.ok_or("--socket is required")?,
    })
}

fn read(path: &PathBuf) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn device_seed(path: &PathBuf) -> Result<[u8; 32], String> {
    let mode = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(format!(
            "{} is readable by others (mode {:o}); it must be owner-only",
            path.display(),
            mode & 0o777
        ));
    }
    decode(read(path)?.trim())?
        .try_into()
        .map_err(|_| "the device seed must be 32 bytes of hexadecimal".to_string())
}

fn run() -> Result<(), String> {
    let args = args()?;
    let trusted = decode(TRUSTED_AUTHORITY_PUBLIC)?;
    let (policy, catalog) =
        parse_charter_config(&read(&args.charter)?).map_err(|e| e.to_string())?;
    let charter = SignedCharter {
        policy,
        catalog,
        signature: decode(read(&args.signature)?.trim())?,
    };
    charter
        .verify(&trusted)
        .map_err(|_| "the charter does not carry the trusted authority's signature".to_string())?;
    let device = keypair_from_seed(&device_seed(&args.device_seed)?).map_err(|e| e.to_string())?;

    let store = ChainStore::open(&args.store).map_err(|e| e.to_string())?;
    let enforcer = if store.exists() {
        let (genesis, steps) = store.load().map_err(|e| e.to_string())?;
        let e = Enforcer::resume(device, charter, &trusted, &genesis, &steps)
            .map_err(|e| format!("the stored chain was refused: {e}"))?;
        eprintln!(
            "resumed at step {} after auditing every step",
            e.state().sequence
        );
        e
    } else {
        let e = Enforcer::genesis(device, charter, &trusted).map_err(|e| e.to_string())?;
        store
            .begin(&e.state(), e.device_public())
            .map_err(|e| e.to_string())?;
        eprintln!("began a new chain");
        e
    };

    if args.socket.exists() {
        std::fs::remove_file(&args.socket).map_err(|e| e.to_string())?;
    }
    let listener = UnixListener::bind(&args.socket).map_err(|e| format!("bind: {e}"))?;
    std::fs::set_permissions(&args.socket, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    eprintln!(
        "listening on {} at tip {}",
        args.socket.display(),
        to_hex(&enforcer.tip())
    );
    Service::new(enforcer, store, trusted)
        .serve(listener)
        .map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
