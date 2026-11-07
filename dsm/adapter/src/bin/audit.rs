//! Audit a stored chain with public material only: the authority's and the device's public keys.
//! The store carries every charter that governed it, and each is checked against the authority.
//! Each step and each charter update is checked from its one parent and replayed through core's
//! transition.
//!
//! ethical-audit --store DIR --authority PUBLIC --device PUBLIC [--world DIR]
//!
//! The device's public key comes from you, like the authority's. The store's own copy is only
//! compared with it. With `--world`, every file there must be exactly what a committed step's
//! receipt says it wrote, and nothing else may be there.

use ethical_autonomy_adapter::act::check_world;
use ethical_autonomy_adapter::hexser::to_hex;
use ethical_autonomy_adapter::keys::read_public_key;
use ethical_autonomy_adapter::store::ChainStore;
use ethical_autonomy_adapter::{audit_chain, check_considered, check_receipts, Link};
use std::path::PathBuf;
use std::process::ExitCode;

fn run() -> Result<(), String> {
    let mut a = std::env::args().skip(1);
    let (mut store, mut world, mut authority, mut device_key) = (None, None, None, None);
    while let Some(flag) = a.next() {
        let value = a
            .next()
            .map(PathBuf::from)
            .ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--store" => store = Some(value),
            "--world" => world = Some(value),
            "--authority" => authority = Some(value),
            "--device" => device_key = Some(value),
            other => return Err(format!("unknown flag {other}")),
        }
    }
    let trusted =
        read_public_key(&authority.ok_or("--authority is required")?).map_err(|e| e.to_string())?;
    let device =
        read_public_key(&device_key.ok_or("--device is required")?).map_err(|e| e.to_string())?;
    let store = ChainStore::open_read_only(&store.ok_or("--store is required")?)
        .map_err(|e| e.to_string())?;
    if store.device_public().map_err(|e| e.to_string())? != device {
        return Err("the store was made by a different device key from the one given".to_string());
    }
    let stored = store.load().map_err(|e| e.to_string())?;
    let links = &stored.links;
    let audit = audit_chain(
        &device,
        &stored.genesis_charter,
        &trusted,
        &stored.genesis,
        links,
    )
    .map_err(|e| e.to_string())?;
    let pending = check_receipts(&device, links, &stored.receipts).map_err(|e| e.to_string())?;
    if pending > 0 {
        return Err(format!(
            "{pending} committed steps have not had their act carried out yet; start the enforcer to finish them"
        ));
    }
    let (set_aside, set_aside_refused) = check_considered(
        &device,
        &audit.charters,
        &stored.genesis,
        links,
        &stored.considered,
    )
    .map_err(|e| e.to_string())?;

    let (mut considered, mut refused, mut steps, mut updates, mut looks) = (0, 0, 0, 0, 0);
    let mut forks = audit.forks.iter();
    for (k, link) in links.iter().enumerate() {
        match link {
            Link::Step(step) => {
                let (size, r, u) = forks.next().copied().unwrap_or((0, 0, 0));
                considered += size;
                refused += r + u;
                steps += 1;
                let what = step.proposal.body.as_ref().map_or_else(
                    || step.proposal.action.clone(),
                    |b| serde_json::to_string(b).unwrap_or_default(),
                );
                println!(
                    "step {:>3}: chose {what} of {size} candidates ({r} refused, {u} unmeasured)  verified",
                    step.successor.sequence
                );
            }
            Link::Observed(o) => {
                looks += 1;
                println!(
                    "step {:>3}: saw {} cells{}  verified",
                    o.successor.sequence,
                    o.observation.seen.len(),
                    if o.observation.marker_seen {
                        String::new()
                    } else {
                        " - its senses failed their check".to_string()
                    }
                );
            }
            Link::Update(update) => {
                updates += 1;
                println!(
                    "step {:>3}: charter moved from version {} to {}, keeping the foundation  verified",
                    update.successor.sequence,
                    audit.charters[k].policy.version,
                    audit.charters[k + 1].policy.version
                );
            }
        }
    }
    println!("genesis verified against core's genesis head");
    println!("{steps} steps verified, each from its parent alone, and replayed through core");
    if looks > 0 {
        println!("{looks} observations verified, each answering the look its parent asked for");
    }
    if updates > 0 {
        println!(
            "{updates} charter {} verified, each signed by the authority and keeping the foundation",
            if updates == 1 { "update" } else { "updates" }
        );
    }
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
    println!(
        "charter in force: version {}",
        audit.current().policy.version
    );
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
