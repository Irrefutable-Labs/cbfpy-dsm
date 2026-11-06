//! The enforcer service. It holds the device key, the chain and the DSM head, and listens on a
//! Unix socket that only its own user can open. A model never runs in this process.
//!
//! ethical-enforcer --store DIR --world DIR --charter C --signature SIG --authority PUBLIC
//!                  --device-seed FILE --socket PATH [--agent-uid UID]
//!
//! With `--agent-uid`, only that user may use the socket, and it may not be the enforcer's own:
//! the AI then cannot read the device seed, the store or the world's files by any route but
//! this one. Without it, the enforcer runs in development mode, sharing its user with the AI.
//!
//! The world directory is where committed actions take effect, and nowhere else.
//!
//! The device seed file must be readable by its owner alone. A store that exists is audited in
//! full and replayed through core before anything is served. One that does not verify stops it.

use ethical_autonomy_adapter::hexser::{decode, to_hex};
use ethical_autonomy_adapter::keys::{read_public_key, read_secret_seed};
use ethical_autonomy_adapter::server::Service;
use ethical_autonomy_adapter::store::ChainStore;
use ethical_autonomy_adapter::{
    check_receipts, keypair_from_seed, parse_charter_config, Enforcer, SignedCharter,
};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::ExitCode;

struct Args {
    store: PathBuf,
    world: PathBuf,
    charter: PathBuf,
    signature: PathBuf,
    device_seed: PathBuf,
    socket: PathBuf,
    authority: PathBuf,
    agent_uid: Option<u32>,
}

fn args() -> Result<Args, String> {
    let mut a = std::env::args().skip(1);
    let (mut store, mut world, mut charter, mut signature, mut seed, mut socket) =
        (None, None, None, None, None, None);
    let (mut authority, mut agent_uid) = (None, None);
    while let Some(flag) = a.next() {
        let value = a
            .next()
            .map(PathBuf::from)
            .ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--store" => store = Some(value),
            "--world" => world = Some(value),
            "--charter" => charter = Some(value),
            "--signature" => signature = Some(value),
            "--device-seed" => seed = Some(value),
            "--socket" => socket = Some(value),
            "--authority" => authority = Some(value),
            "--agent-uid" => {
                agent_uid = Some(
                    value
                        .to_str()
                        .and_then(|v| v.parse().ok())
                        .ok_or("--agent-uid needs a number")?,
                )
            }
            other => return Err(format!("unknown flag {other}")),
        }
    }
    Ok(Args {
        store: store.ok_or("--store is required")?,
        world: world.ok_or("--world is required")?,
        charter: charter.ok_or("--charter is required")?,
        signature: signature.ok_or("--signature is required")?,
        device_seed: seed.ok_or("--device-seed is required")?,
        socket: socket.ok_or("--socket is required")?,
        authority: authority.ok_or("--authority is required")?,
        agent_uid,
    })
}

fn read(path: &PathBuf) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn run() -> Result<(), String> {
    let args = args()?;
    let trusted = read_public_key(&args.authority).map_err(|e| e.to_string())?;
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
    let device =
        keypair_from_seed(&read_secret_seed(&args.device_seed).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;

    let store = ChainStore::open(&args.store).map_err(|e| e.to_string())?;
    let (enforcer, steps, receipts) = if store.exists() {
        let stored = store.load().map_err(|e| e.to_string())?;
        let e = Enforcer::resume(device, charter, &trusted, &stored.genesis, &stored.steps)
            .map_err(|e| format!("the stored chain was refused: {e}"))?;
        check_receipts(e.device_public(), &stored.steps, &stored.receipts)
            .map_err(|e| format!("the stored chain was refused: {e}"))?;
        eprintln!(
            "resumed at step {} after auditing every step",
            e.state().sequence
        );
        (e, stored.steps, stored.receipts.len())
    } else {
        let e = Enforcer::genesis(device, charter, &trusted).map_err(|e| e.to_string())?;
        store
            .begin(&e.state(), e.device_public())
            .map_err(|e| e.to_string())?;
        eprintln!("began a new chain");
        (e, vec![], 0)
    };
    let tip = enforcer.tip();
    let mut service = Service::start(
        enforcer,
        store,
        args.world.clone(),
        trusted,
        &steps,
        receipts,
    )
    .map_err(|e| e.to_string())?;
    let socket_mode = match args.agent_uid {
        Some(uid) => {
            service = service.only_for(uid).map_err(|e| e.to_string())?;
            eprintln!("serving only user {uid}");
            // Anyone may connect; the operating system's word on who they are decides.
            0o666
        }
        None => {
            eprintln!(
                "development mode: the AI shares this user, so nothing but convention keeps it from the key"
            );
            0o600
        }
    };

    if args.socket.exists() {
        std::fs::remove_file(&args.socket).map_err(|e| e.to_string())?;
    }
    let listener = UnixListener::bind(&args.socket).map_err(|e| format!("bind: {e}"))?;
    std::fs::set_permissions(&args.socket, std::fs::Permissions::from_mode(socket_mode))
        .map_err(|e| e.to_string())?;
    eprintln!(
        "listening on {} at tip {}",
        args.socket.display(),
        to_hex(&tip)
    );
    service.serve(listener).map_err(|e| e.to_string())
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
