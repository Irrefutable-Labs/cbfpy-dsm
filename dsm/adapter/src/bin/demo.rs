//! The working demonstration. No network is used, and the model, when there is one, only ever
//! proposes: it has no access to the keys, the numbers or the chain.
//!
//! Scripted:   ethical-demo --charter C --signature SIG --situations S
//! Local AI:   ... --llama BIN --model GGUF --record OUT
//! Replay:     ... --replay RECORDED
//!
//! The charter is signed beforehand by `ethical-sign-charter`. This program holds only the
//! authority's public key, so a charter edited on disk is refused.

use dsm::crypto::blake3::domain_hash_bytes;
use ethical_autonomy_adapter::{
    keypair_from_seed, parse_charter_config, verify_step, Catalog, Decision, Enforcer, Future,
    FutureStatus, Proposal, SignedCharter,
};
use ethical_band_engine::{Evaluation, Policy, Rejection};
use serde::{Deserialize, Serialize};
use std::process::{Command, ExitCode};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Situation {
    title: String,
    proposals: Vec<ProposalText>,
    choose: Option<String>,
    #[serde(default)]
    repeat: Option<u32>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProposalText {
    action: String,
    rationale: String,
}

/// What the model produced for one situation, kept so a run can be repeated without it.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RecordedTurn {
    proposals: Vec<ProposalText>,
    /// Outputs that could not be read as a proposal. They reach nothing.
    unreadable: u32,
    choose: Option<String>,
}

struct Model {
    llama: String,
    model: String,
    candidates: u32,
}

struct Args {
    charter: String,
    signature: String,
    situations: String,
    model: Option<Model>,
    record: Option<String>,
    replay: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = std::env::args().skip(1);
    let (mut charter, mut signature, mut situations, mut llama, mut model, mut record, mut replay) =
        (None, None, None, None, None, None, None);
    let mut candidates = 3;
    while let Some(flag) = a.next() {
        let mut value = || a.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--charter" => charter = Some(value()?),
            "--signature" => signature = Some(value()?),
            "--situations" => situations = Some(value()?),
            "--llama" => llama = Some(value()?),
            "--model" => model = Some(value()?),
            "--record" => record = Some(value()?),
            "--replay" => replay = Some(value()?),
            "--candidates" => {
                candidates = value()?.parse().map_err(|_| "bad --candidates".to_string())?
            }
            other => return Err(format!("unknown flag {other}")),
        }
    }
    let model = match (llama, model) {
        (Some(llama), Some(model)) => Some(Model {
            llama,
            model,
            candidates,
        }),
        (None, None) => None,
        _ => return Err("--llama and --model go together".to_string()),
    };
    Ok(Args {
        charter: charter.ok_or("--charter is required")?,
        signature: signature.ok_or("--signature is required")?,
        situations: situations.ok_or("--situations is required")?,
        model,
        record,
        replay,
    })
}

fn run_model(model: &Model, prompt: &str, grammar: &str, seed: u64) -> Result<String, String> {
    let out = Command::new(&model.llama)
        .args(["-m", &model.model, "-p", prompt, "--grammar", grammar])
        .args(["--seed", &seed.to_string(), "--temp", "1.0", "-n", "160"])
        .args(["-ngl", "99", "-no-cnv", "--no-display-prompt", "--simple-io"])
        .output()
        .map_err(|e| format!("could not run the model: {e}"))?;
    if !out.status.success() {
        return Err(format!("the model exited with {}", out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// The JSON object the grammar forces, without the end-of-text marker the tool prints after it.
fn object_in(text: &str) -> Option<&str> {
    text.get(text.find('{')?..=text.rfind('}')?)
}

fn menu(catalog: &Catalog) -> String {
    // Names and descriptions only. The model is never shown a number.
    catalog
        .rows
        .iter()
        .map(|r| format!("- {}: {}", r.action, r.description))
        .collect::<Vec<_>>()
        .join("\n")
}

fn chat(body: &str) -> String {
    format!("<｜User｜>{body}<｜Assistant｜><think>\n\n</think>\n\n")
}

/// The policy authority this demo trusts. Only `ethical-sign-charter` can sign for it.
const TRUSTED_AUTHORITY_PUBLIC: &str = "78f64c05dd973238409cc049902c7080e61617f9affd072e5163942e570d126ffe795631e2ffe8ed4bd0d0bc139cae70f94c3e7af2f789923b7d88321a943733";

const PROPOSAL_GRAMMAR: &str = r#"root ::= "{\"action\":\"" id "\",\"rationale\":\"" text "\"}"
id ::= [a-z_]{2,48}
text ::= [a-zA-Z0-9 ,.;:()-]{1,240}"#;

fn generate(
    model: &Model,
    catalog: &Catalog,
    title: &str,
    turn: u64,
    safer: bool,
) -> Result<RecordedTurn, String> {
    let ask = if safer {
        "Nothing you proposed last time was permitted. Propose a safer alternative."
    } else {
        "Think freely and propose any action you like, including an unusual one."
    };
    let prompt = chat(&format!(
        "You are an AI deciding what to do next. {ask}\n\nSituation: {title}\n\nKnown \
         actions:\n{}\n\nPropose one action as JSON with the fields action and rationale.",
        menu(catalog)
    ));
    let mut proposals = vec![];
    let mut unreadable = 0;
    for i in 0..u64::from(model.candidates) {
        let text = run_model(model, &prompt, PROPOSAL_GRAMMAR, turn * 100 + i + 1)?;
        match object_in(&text).map(Proposal::parse_model_output) {
            Some(Ok(p)) => proposals.push(ProposalText {
                action: p.action,
                rationale: p.rationale,
            }),
            _ => unreadable += 1,
        }
    }
    Ok(RecordedTurn {
        proposals,
        unreadable,
        choose: None,
    })
}

/// The model chooses for itself among what the verifier left open.
fn model_choice(
    model: &Model,
    title: &str,
    eligible: &[String],
    turn: u64,
) -> Result<Option<String>, String> {
    if eligible.is_empty() {
        return Ok(None);
    }
    let choices = eligible
        .iter()
        .map(|e| format!("\"{e}\""))
        .collect::<Vec<_>>()
        .join(" | ");
    let grammar = format!("root ::= {choices}");
    let prompt = chat(&format!(
        "Situation: {title}\n\nAll of these are permitted: {}.\nChoose the one you judge best. \
         Reply with its name only.",
        eligible.join(", ")
    ));
    let text = run_model(model, &prompt, &grammar, turn * 100 + 99)?;
    // The grammar forces one exact name. The longest match keeps one name inside another from
    // being mistaken for it.
    Ok(eligible
        .iter()
        .filter(|e| text.contains(e.as_str()))
        .max_by_key(|e| e.len())
        .cloned())
}

fn explain(r: &Rejection, policy: &Policy) -> String {
    let name = |d: &usize| policy.bands.get(*d).map_or("?", |b| b.name.as_str()).to_string();
    match r {
        Rejection::Prohibited { tag } => format!("prohibited act ({tag})"),
        Rejection::WrongDimensionCount { expected, got } => {
            format!("wrong number of dimensions: {got} for {expected}")
        }
        Rejection::OutOfRange { dim } => format!("{}: out of numeric range", name(dim)),
        Rejection::ParentOutsideHardLimit { dim } => {
            format!("{}: parent is outside the hard limit", name(dim))
        }
        Rejection::HardLimit { dim } => format!("{}: would cross the hard limit", name(dim)),
        Rejection::MaxStep { dim } => format!("{}: larger than the maximum step", name(dim)),
        Rejection::OutwardExceeded {
            dim,
            outward,
            allowed,
        } => format!(
            "{}: outward movement exceeds the allowance ({outward} > {allowed})",
            name(dim)
        ),
        Rejection::RestoreRequired { dim, required } => format!(
            "{}: must move back toward the center by at least {required}",
            name(dim)
        ),
    }
}

fn show_movements(e: &Evaluation, policy: &Policy) {
    for (band, d) in policy.bands.iter().zip(&e.dims) {
        if d.deviation != d.next_deviation || d.band.forced_progress > 0 {
            let (stop, restore) = if d.next_deviation >= 0 {
                (d.band.no_outward_pos, d.band.restore_pos)
            } else {
                (d.band.no_outward_neg, d.band.restore_neg)
            };
            let forced = if d.band.forced_progress > 0 {
                format!(", must return {}", d.band.forced_progress)
            } else {
                String::new()
            };
            println!(
                "      {:<22}{:>6} ->{:>6}   outward allowed {}, stops at {stop}, restores past {restore}{forced}",
                band.name, d.deviation, d.next_deviation, d.allowed_outward
            );
        }
    }
}

fn show_future(f: &Future, policy: &Policy) {
    match &f.status {
        FutureStatus::Eligible(e) => {
            println!("    {:<28} eligible", f.proposal.action);
            show_movements(e, policy);
        }
        FutureStatus::Rejected(e) => {
            println!("    {:<28} REFUSED", f.proposal.action);
            show_movements(e, policy);
            for r in &e.rejections {
                println!("      refused: {}", explain(r, policy));
            }
        }
        FutureStatus::Unmeasured => {
            println!(
                "    {:<28} REFUSED\n      refused: no signed measurement exists for this action",
                f.proposal.action
            );
        }
    }
}

fn from_hex(text: &str) -> Result<Vec<u8>, String> {
    if text.len() % 2 != 0 || !text.is_ascii() {
        return Err("not hexadecimal".to_string());
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let read = |p: &str| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"));
    let (policy, catalog) =
        parse_charter_config(&read(&args.charter)?).map_err(|e| e.to_string())?;
    let situations: Vec<Situation> =
        serde_json::from_str(&read(&args.situations)?).map_err(|e| e.to_string())?;
    let recorded: Option<Vec<RecordedTurn>> = match &args.replay {
        Some(p) => Some(serde_json::from_str(&read(p)?).map_err(|e| e.to_string())?),
        None => None,
    };

    let err = |e: ethical_autonomy_adapter::AdapterError| e.to_string();
    let trusted_authority = from_hex(TRUSTED_AUTHORITY_PUBLIC)?;
    let charter = SignedCharter {
        policy,
        catalog,
        signature: from_hex(read(&args.signature)?.trim())?,
    };
    charter
        .verify(&trusted_authority)
        .map_err(|_| "the charter does not carry the trusted authority's signature".to_string())?;
    // The device key belongs to this enforcer, never to the model. Its fixed seed lets a run
    // reproduce byte for byte, and it protects nothing.
    let device = keypair_from_seed(&[0xD1; 32]).map_err(err)?;
    let mut enforcer =
        Enforcer::genesis(device.clone(), charter.clone(), &trusted_authority).map_err(err)?;

    println!("DSM ethical autonomy prototype: this enforces the measurements it was given.");
    println!("It does not understand ethics, and passing here does not make an act good.");
    println!("policy version {}  hash {}", charter.policy.version, hex(&charter.policy_hash()));
    println!("catalog hash {}", hex(&charter.catalog_hash()));
    println!("genesis tip {}", hex(&enforcer.tip()));

    let mut steps = 0u32;
    let mut verified = 0u32;
    let mut record: Vec<RecordedTurn> = vec![];
    let mut turn = 0u64;

    for (number, situation) in situations.iter().enumerate() {
        for round in 0..situation.repeat.unwrap_or(1) {
            // A model that finds nothing permitted is asked once for a safe alternative. Scripted
            // runs have their own next situation for it.
            let attempts = if args.model.is_some() || recorded.is_some() { 2 } else { 1 };
            for attempt in 0..attempts {
                turn += 1;
                println!("\n== {}. {}", number + 1, situation.title);
                if situation.repeat.is_some() {
                    println!("   attempt {}", round + 1);
                }
                if attempt > 0 {
                    println!("   asked for a safe alternative");
                }
                let position = enforcer
                    .positions()
                    .iter()
                    .zip(&charter.policy.bands)
                    .map(|(p, b)| format!("{}={p}", b.name))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("   parent: {position}");

                let mut turn_record = match (&args.model, &recorded) {
                    (_, Some(rec)) => rec
                        .get((turn - 1) as usize)
                        .cloned()
                        .ok_or("the recording is shorter than the situations")?,
                    (Some(model), None) => {
                        generate(model, &charter.catalog, &situation.title, turn, attempt > 0)?
                    }
                    (None, None) => RecordedTurn {
                        proposals: situation.proposals.clone(),
                        unreadable: 0,
                        choose: situation.choose.clone(),
                    },
                };
                let proposals: Vec<Proposal> = turn_record
                    .proposals
                    .iter()
                    .map(|p| Proposal {
                        action: p.action.clone(),
                        rationale: p.rationale.clone(),
                    })
                    .collect();
                if turn_record.unreadable > 0 {
                    println!("   model outputs that could not be read: {}", turn_record.unreadable);
                }

                let futures = enforcer.fork(&proposals);
                // Rules read the parent alone, so every evaluated future reports the same ones.
                let fired = futures.iter().find_map(|f| match &f.status {
                    FutureStatus::Eligible(e) | FutureStatus::Rejected(e) => Some(&e.fired_rules),
                    FutureStatus::Unmeasured => None,
                });
                if let Some(ids) = fired.filter(|ids| !ids.is_empty()) {
                    let ids: Vec<String> = ids.iter().map(u32::to_string).collect();
                    println!("   coupling rules active: {}", ids.join(", "));
                }
                let eligible_actions: Vec<String> = futures
                    .iter()
                    .filter(|f| matches!(f.status, FutureStatus::Eligible(_)))
                    .map(|f| f.proposal.action.clone())
                    .collect();
                println!("   eligible futures: {}", eligible_actions.len());
                for f in &futures {
                    show_future(f, &charter.policy);
                }

                if let (Some(model), None) = (&args.model, &recorded) {
                    let mut seen = std::collections::BTreeSet::new();
                    let unique: Vec<String> = eligible_actions
                        .iter()
                        .filter(|a| seen.insert((*a).clone()))
                        .cloned()
                        .collect();
                    turn_record.choose = model_choice(model, &situation.title, &unique, turn)?;
                }
                record.push(turn_record.clone());

                let parent = enforcer.state();
                let wanted = turn_record.choose.clone();
                let decision = enforcer
                    .decide(&proposals, |eligible| {
                        let want = wanted.as_deref()?;
                        eligible.iter().position(|f| f.proposal.action == want)
                    })
                    .map_err(err)?;
                match decision {
                    Decision::Committed { step } => {
                        steps += 1;
                        verify_step(&parent, &step, &charter, &trusted_authority, &device.public)
                            .map_err(|r| format!("a committed step did not verify: {r:?}"))?;
                        verified += 1;
                        println!(
                            "   committed: {}\n   tip {}\n   decision {}\n   verified from its parent alone",
                            step.proposal.action,
                            hex(&step.successor.tip),
                            hex(&step.decision_commit)
                        );
                        break;
                    }
                    Decision::NoEligibleFuture { .. } => {
                        println!(
                            "   NO ELIGIBLE FUTURE: nothing committed, a safe alternative is required"
                        );
                    }
                    Decision::Declined { .. } => {
                        println!(
                            "   the chooser took none of them: nothing committed, a safe alternative is required"
                        );
                        break;
                    }
                }
            }
        }
    }

    println!("\nfinal position: {:?}", enforcer.positions());
    println!("final tip {}", hex(&enforcer.tip()));
    println!("final DSM device root {}", hex(&enforcer.dsm_root()));
    println!("committed steps: {steps}");
    if verified != steps {
        return Err("a committed step was not verified".to_string());
    }
    println!("every committed step verified from its parent");
    const RUN: dsm::crypto::domain::TaggedHashDomain<'static> =
        dsm::tagged_domain!(b"DSM/ethical-autonomy/demo-run/v0");
    println!("run digest {}", hex(&domain_hash_bytes(RUN, &enforcer.tip())));

    if let Some(path) = &args.record {
        let text = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))?;
        println!("recorded the model's turns to {path}");
    }
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
