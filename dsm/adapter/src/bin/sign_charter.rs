//! The policy authority's own tool, kept apart from anything that runs the AI.
//!
//! ethical-sign-charter keygen --key SEED_FILE --public PUBLIC_FILE
//! ethical-sign-charter sign --key SEED_FILE --charter scenarios/charter.json --out scenarios/charter.sig
//!
//! The seed is random and readable by its owner alone. Keep it off the machine that runs the AI.
//! The enforcer and the audit are given only the public key.

use ethical_autonomy_adapter::hexser::to_hex;
use ethical_autonomy_adapter::keys::{read_secret_seed, write_new_seed};
use ethical_autonomy_adapter::{keypair_from_seed, parse_charter_config, SignedCharter};
use std::path::PathBuf;
use std::process::ExitCode;

fn run() -> Result<(), String> {
    let mut a = std::env::args().skip(1);
    let command = a.next().ok_or("keygen or sign")?;
    let (mut key, mut public, mut charter, mut out) = (None, None, None, None);
    while let Some(flag) = a.next() {
        let value = a
            .next()
            .map(PathBuf::from)
            .ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--key" => key = Some(value),
            "--public" => public = Some(value),
            "--charter" => charter = Some(value),
            "--out" => out = Some(value),
            other => return Err(format!("unknown flag {other}")),
        }
    }
    let key = key.ok_or("--key is required")?;
    match command.as_str() {
        "keygen" => {
            let public = public.ok_or("--public is required")?;
            let seed = write_new_seed(&key).map_err(|e| e.to_string())?;
            let authority = keypair_from_seed(&seed).map_err(|e| e.to_string())?;
            std::fs::write(&public, to_hex(&authority.public)).map_err(|e| e.to_string())?;
            println!(
                "wrote a new authority seed to {}, readable by you alone",
                key.display()
            );
            println!("wrote its public key to {}", public.display());
            Ok(())
        }
        "sign" => {
            let charter = charter.ok_or("--charter is required")?;
            let out = out.ok_or("--out is required")?;
            let text = std::fs::read_to_string(&charter)
                .map_err(|e| format!("{}: {e}", charter.display()))?;
            let (policy, catalog) = parse_charter_config(&text).map_err(|e| e.to_string())?;
            let seed = read_secret_seed(&key).map_err(|e| e.to_string())?;
            let authority = keypair_from_seed(&seed).map_err(|e| e.to_string())?;
            let signed =
                SignedCharter::sign(policy, catalog, &authority).map_err(|e| e.to_string())?;
            std::fs::write(&out, to_hex(&signed.signature)).map_err(|e| e.to_string())?;
            println!("signed {} with the authority key", charter.display());
            println!("policy hash {}", to_hex(&signed.policy_hash()));
            Ok(())
        }
        other => Err(format!("unknown command {other}: keygen or sign")),
    }
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
