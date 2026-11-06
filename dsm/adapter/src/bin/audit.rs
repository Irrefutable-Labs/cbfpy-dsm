//! Audit a stored chain with public material only: the charter, its signature, and the device's
//! public key the store recorded. Each step is checked from its one parent and replayed through
//! core's transition.
//!
//! ethical-audit --store DIR --charter C --signature SIG

use ethical_autonomy_adapter::hexser::{decode, to_hex};
use ethical_autonomy_adapter::store::ChainStore;
use ethical_autonomy_adapter::{
    audit_chain, parse_charter_config, SignedCharter, TRUSTED_AUTHORITY_PUBLIC,
};
use std::path::PathBuf;
use std::process::ExitCode;

fn run() -> Result<(), String> {
    let mut a = std::env::args().skip(1);
    let (mut store, mut charter, mut signature) = (None, None, None);
    while let Some(flag) = a.next() {
        let value = a
            .next()
            .map(PathBuf::from)
            .ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--store" => store = Some(value),
            "--charter" => charter = Some(value),
            "--signature" => signature = Some(value),
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
    let device = store.device_public().map_err(|e| e.to_string())?;
    let (genesis, steps) = store.load().map_err(|e| e.to_string())?;
    let trusted = decode(TRUSTED_AUTHORITY_PUBLIC)?;
    let audit =
        audit_chain(&device, &charter, &trusted, &genesis, &steps).map_err(|e| e.to_string())?;
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
    println!("{considered} candidates considered, {refused} of them refused or unmeasured");
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
