//! Audit a stored chain with public material only: the charter, its signature, and the device's
//! public key the store recorded. Each step is checked from its one parent and replayed through
//! core's transition.
//!
//! ethical-audit --store DIR --charter C --signature SIG --authority PUBLIC --device PUBLIC [--world DIR]
//!
//! The device's public key comes from you, like the authority's. The store's own copy is only
//! compared with it.
//!
//! With `--world`, every file there must be exactly what a committed step's receipt says it
//! wrote, and nothing else may be there.

use ethical_autonomy_adapter::act::check_world;
use ethical_autonomy_adapter::hexser::{decode, to_hex};
use ethical_autonomy_adapter::keys::read_public_key;
use ethical_autonomy_adapter::store::ChainStore;
use ethical_autonomy_adapter::{
    audit_chain, check_considered, check_receipts, parse_charter_config, SignedCharter,
};
use std::path::PathBuf;
use std::process::ExitCode;

fn run() -> Result<(), String> {
    let mut a = std::env::args().skip(1);
    let (mut store, mut charter, mut signature, mut world, mut authority) =
        (None, None, None, None, None);
    let mut device_key = None;
    while let Some(flag) = a.next() {
        let value = a
            .next()
            .map(PathBuf::from)
            .ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--store" => store = Some(value),
            "--charter" => charter = Some(value),
            "--signature" => signature = Some(value),
            "--world" => world = Some(value),
            "--authority" => authority = Some(value),
            "--device" => device_key = Some(value),
            other => return Err(format!("unknown flag {other}")),
        }
    }
    let read =
        |p: &PathBuf| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let charter_path = charter.ok_or("--charter is required")?;
    let (policy, catalog) =
        parse_charter_config(&read(&charter_path)?).map_err(|e| e.to_string())?;
    let charter = SignedCharter {
        policy,
        catalog,
        signature: decode(read(&signature.ok_or("--signature is required")?)?.trim())?,
    };
    let store =
        ChainStore::open(&store.ok_or("--store is required")?).map_err(|e| e.to_string())?;
    let device =
        read_public_key(&device_key.ok_or("--device is required")?).map_err(|e| e.to_string())?;
    if store.device_public().map_err(|e| e.to_string())? != device {
        return Err("the store was made by a different device key from the one given".to_string());
    }
    let stored = store.load().map_err(|e| e.to_string())?;
    let steps = stored.steps;
    let trusted =
        read_public_key(&authority.ok_or("--authority is required")?).map_err(|e| e.to_string())?;
    let audit = audit_chain(&device, &charter, &trusted, &stored.genesis, &steps)
        .map_err(|e| e.to_string())?;
    let pending = check_receipts(&device, &steps, &stored.receipts).map_err(|e| e.to_string())?;
    let (set_aside, set_aside_refused) = check_considered(
        &device,
        &charter,
        &stored.genesis,
        &steps,
        &stored.considered,
    )
    .map_err(|e| e.to_string())?;
    if pending > 0 {
        return Err(format!(
            "{pending} committed steps have not had their act carried out yet; start the enforcer to finish them"
        ));
    }
    let mut considered = 0;
    let mut refused = 0;
    for (n, (step, (size, r, u))) in steps.iter().zip(&audit.forks).enumerate() {
        considered += size;
        refused += r + u;
        println!(
            "step {:>3}: chose {:<28} of {} candidates ({} refused, {} unmeasured)  verified",
            n + 1,
            step.proposal.action,
            size,
            r,
            u
        );
    }
    println!("genesis verified against core's genesis head");
    println!(
        "{} steps verified, each from its parent alone, and replayed through core",
        steps.len()
    );
    println!("{considered} candidates in committed forks, {refused} of them refused or unmeasured");
    println!(
        "{} superseded forks verified from their parents, holding {set_aside} more candidates, {set_aside_refused} of them refused or unmeasured",
        stored.considered.len()
    );
    println!("every step's act has a receipt signed by the device");
    if let Some(world) = world {
        let files = check_world(&world, &stored.receipts).map_err(|e| e.to_string())?;
        println!(
            "the world holds exactly the {files} files committed actions wrote, and nothing else"
        );
    }
    println!("final tip {}", to_hex(&audit.last.tip));
    println!("final DSM device root {}", to_hex(&audit.dsm_root));
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("audit failed: {e}");
            ExitCode::FAILURE
        }
    }
}
