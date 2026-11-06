//! The policy authority's own step, separate from anything that runs the AI: it signs a charter.
//! The demo only ever verifies, against a public key pinned in its source.
//!
//! ethical-sign-charter --charter scenarios/charter.json --out scenarios/charter.sig
//!
//! The prototype's authority key comes from a fixed seed, so signatures reproduce. It protects
//! nothing, and a real authority's key would never sit in source.

use ethical_autonomy_adapter::{keypair_from_seed, parse_charter_config, SignedCharter};
use std::process::ExitCode;

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (mut charter, mut out) = (None, None);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--charter" => charter = Some(value),
            "--out" => out = Some(value),
            other => return Err(format!("unknown flag {other}")),
        }
    }
    let charter = charter.ok_or("--charter is required")?;
    let out = out.ok_or("--out is required")?;
    let text = std::fs::read_to_string(&charter).map_err(|e| format!("{charter}: {e}"))?;
    let (policy, catalog) = parse_charter_config(&text).map_err(|e| e.to_string())?;
    let authority = keypair_from_seed(&[0xA1; 32]).map_err(|e| e.to_string())?;
    let signed = SignedCharter::sign(policy, catalog, &authority).map_err(|e| e.to_string())?;
    let hex: String = signed.signature.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::write(&out, hex).map_err(|e| format!("{out}: {e}"))?;
    let public: String = authority.public.iter().map(|b| format!("{b:02x}")).collect();
    println!("authority public key {public}");
    println!("policy hash {}", signed.policy_hash().iter().map(|b| format!("{b:02x}")).collect::<String>());
    Ok(())
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
