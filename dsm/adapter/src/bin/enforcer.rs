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
//! A store carries the charter it began under and every update since. For an existing store,
//! `--charter` must be the charter it is under now. To move it to a new operational charter,
//! signed by the authority and keeping the foundation unchanged, start with `--apply-update`;
//! the update is recorded on the chain before anything is served. The AI has no route to this.
//!
//! The device seed file must be readable by its owner alone. A store that exists is audited in
//! full and replayed through core before anything is served. One that does not verify stops it.

use ethical_autonomy_adapter::hexser::{decode, to_hex};
use ethical_autonomy_adapter::keys::{read_public_key, read_secret_seed};
use ethical_autonomy_adapter::server::Service;
use ethical_autonomy_adapter::store::ChainStore;
use ethical_autonomy_adapter::{
    check_receipts, keypair_from_seed, parse_charter_config, pending_steps, Enforcer, SignedCharter,
};
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
    apply_update: bool,
}

fn args() -> Result<Args, String> {
    let mut a = std::env::args().skip(1);
    let (mut store, mut world, mut charter, mut signature, mut seed, mut socket) =
        (None, None, None, None, None, None);
    let (mut authority, mut agent_uid) = (None, None);
    let mut apply_update = false;
    while let Some(flag) = a.next() {
        if flag == "--apply-update" {
            apply_update = true;
            continue;
        }
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
        apply_update,
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

    // Nothing the enforcer keeps may sit where a command can read it.
    std::fs::create_dir_all(&args.world).map_err(|e| e.to_string())?;
    let world = args.world.canonicalize().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&args.store).map_err(|e| e.to_string())?;
    let seed = args
        .device_seed
        .canonicalize()
        .map_err(|e| format!("{}: {e}", args.device_seed.display()))?;
    let store_dir = args.store.canonicalize().map_err(|e| e.to_string())?;
    if seed.starts_with(&world) {
        return Err("the device seed must not be inside the world".to_string());
    }
    if store_dir.starts_with(&world) || world.starts_with(&store_dir) {
        return Err("the store must not be inside the world, or contain it".to_string());
    }
    let store = ChainStore::open(&args.store).map_err(|e| e.to_string())?;
    let _lock = store.lock().map_err(|e| e.to_string())?;
    let (enforcer, pending) = if store.exists() {
        store.drop_torn_tail().map_err(|e| e.to_string())?;
        let stored = store.load().map_err(|e| e.to_string())?;
        let mut e = Enforcer::resume(
            device,
            stored.genesis_charter.clone(),
            &trusted,
            &stored.genesis,
            &stored.links,
        )
        .map_err(|e| format!("the stored chain was refused: {e}"))?;
        check_receipts(e.device_public(), &stored.links, &stored.receipts)
            .map_err(|e| format!("the stored chain was refused: {e}"))?;
        eprintln!(
            "resumed at step {} after auditing every step, under charter version {}",
            e.state().sequence,
            e.charter().policy.version
        );
        let pending = pending_steps(&stored.links, &stored.receipts);
        if charter.policy_hash() != e.charter().policy_hash()
            || charter.catalog_hash() != e.charter().catalog_hash()
        {
            if !args.apply_update {
                return Err(format!(
                    "the store is under charter version {}, not the one given; start with --apply-update to move it to this charter",
                    e.charter().policy.version
                ));
            }
            if !pending.is_empty() {
                return Err(
                    "an act is outstanding; start once without --apply-update to finish it"
                        .to_string(),
                );
            }
            let record = e
                .apply_update(charter, &trusted, |u| store.append_update(u))
                .map_err(|e| format!("the update was refused: {e}"))?;
            eprintln!(
                "moved to charter version {} at step {}, keeping the foundation",
                e.charter().policy.version,
                record.successor.sequence
            );
        } else if args.apply_update {
            return Err(
                "--apply-update was given, but the store is already under this charter".to_string(),
            );
        }
        (e, pending)
    } else {
        let e = Enforcer::genesis(device, charter.clone(), &trusted).map_err(|e| e.to_string())?;
        store
            .begin(&e.state(), e.device_public(), &charter)
            .map_err(|e| e.to_string())?;
        eprintln!("began a new chain");
        (e, vec![])
    };
    let tip = enforcer.tip();
    let mut service = Service::start(enforcer, store, args.world.clone(), trusted, &pending)
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
    // The socket is created with its mode, rather than changed after: nothing can swap the
    // path for a link in between.
    // SAFETY: umask only sets this process's file-creation mask, and it is restored at once.
    let previous = unsafe { libc::umask(0o777 & !socket_mode) };
    let bound = UnixListener::bind(&args.socket);
    // SAFETY: as above.
    unsafe { libc::umask(previous) };
    let listener = bound.map_err(|e| format!("bind: {e}"))?;
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
