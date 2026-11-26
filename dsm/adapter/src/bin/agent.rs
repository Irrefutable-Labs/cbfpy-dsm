//! The AI's side, from the command line. It holds no key and no charter: it reaches the enforcer
//! only through its socket, and can only propose and choose. When everything it proposes is
//! refused, it is told why in words and asked again: read first, do less, ask, or decline.
//!
//! Scripted:  ethical-agent --socket PATH --situations S
//! Local AI:  ethical-agent --socket PATH --situations S --model deepseek-r1:8b
//!            [--ollama 127.0.0.1:11434] [--directory D] [--candidates 3] [--record OUT]
//! Replay:    ethical-agent --socket PATH --situations S --replay RECORDED
//! State:     ethical-agent --socket PATH --status
//!
//! Scripted, each situation is one step: one act committed, or the refusals that stopped it. With a
//! model, or replaying one, each request (a run of situations with the same title) is worked through
//! as steps, hearing what each did, exactly as the app does; the script's own proposals are unused.

use ethical_autonomy_adapter::fork::Outcome;
use ethical_autonomy_adapter::model::Ollama;
use ethical_autonomy_adapter::protocol::{Request, Response};
use ethical_autonomy_adapter::session::{
    run_request, run_step, AttemptOutcome, Choice, Given, Link, Live, Mind, RecordedTurn, Step,
};
use ethical_autonomy_adapter::Proposal;
use ethical_world::knowledge::Knowledge;
use serde::Deserialize;
use std::collections::VecDeque;
use std::process::ExitCode;

/// A live or replayed model is asked again this many times after everything it proposed is
/// refused. Scripted runs have their own next situation for it.
const RETRIES: u32 = 2;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Situation {
    title: String,
    proposals: Vec<Proposal>,
    choose: Option<Choice>,
    #[serde(default)]
    repeat: Option<u32>,
}

/// The facility's data sources, by name: what the model may ask to read. Names only.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Directory {
    sources: Vec<String>,
}

fn knowledge_line(k: &Knowledge) -> String {
    let feeds = match (k.feeds_ok_at, k.feeds_failed_at) {
        (None, _) => "feeds not yet proven".to_string(),
        (Some(ok), Some(failed)) if failed >= ok => "FEEDS FAILED their check".to_string(),
        (Some(ok), _) => format!("feeds proven at step {ok}"),
    };
    format!(
        "{} sources read; {} beings known; {feeds}",
        k.reads.len(),
        k.beings.len()
    )
}

fn print_step(number: usize, title: &str, round: Option<u32>, step: &Step) -> bool {
    let mut committed = false;
    for (n, a) in step.attempts.iter().enumerate() {
        println!("\n== {number}. {title}");
        if let Some(r) = round {
            println!("   attempt {}", r + 1);
        }
        if n > 0 {
            println!("   asked again, with the guardrails' guidance:");
            for g in &a.guidance {
                println!("     - {g}");
            }
        }
        println!("   knows: {}", knowledge_line(&a.knows));
        if a.unreadable > 0 {
            println!("   model outputs that could not be read: {}", a.unreadable);
        }
        let eligible = a
            .futures
            .iter()
            .filter(|f| f.outcome == Outcome::Eligible)
            .count();
        println!("   eligible futures: {eligible}");
        for f in &a.futures {
            let verdict = if f.outcome == Outcome::Eligible {
                "eligible"
            } else {
                "REFUSED"
            };
            let what = f.body.as_ref().map_or(f.action.clone(), |b| {
                serde_json::to_string(b).unwrap_or_default()
            });
            println!("    [{}] {what:<60} {verdict}", f.index);
            if let Some(p) = &f.purpose {
                println!("      declares its purpose: {p}");
            }
            if !f.reach.is_empty() {
                let severity = f
                    .severity
                    .map_or(String::new(), |s| format!(", worst case {s:?}"));
                println!("      affects {}{severity}", f.reach.join(", "));
            }
            for e in &f.effects {
                println!(
                    "      {} of {} ({}): {} -> {}",
                    e.dimension.name(),
                    e.id,
                    e.species,
                    e.from,
                    e.to
                );
            }
            for r in &f.reasons {
                println!("      refused: {r}");
            }
        }
        match &a.outcome {
            AttemptOutcome::NothingAllowed => {
                println!("   NO ELIGIBLE FUTURE: nothing committed, a safe alternative is required")
            }
            AttemptOutcome::Declined => println!(
                "   the chooser took none of them: nothing committed, a safe alternative is required"
            ),
            AttemptOutcome::Refused { reason } => println!("   the enforcer refused it: {reason}"),
            AttemptOutcome::Committed {
                did,
                fork_size,
                tip,
                decision,
                effects,
                observed,
                ..
            } => {
                committed = true;
                println!(
                    "   committed: {did}, with all {fork_size} candidates in the fork\n   tip {tip}\n   decision {decision}\n   verified from its parent alone by the enforcer"
                );
                if effects.is_empty() {
                    println!("   acted: nothing outside the chain");
                }
                for e in effects {
                    println!("   acted: wrote {e}");
                }
                if let Some(o) = observed {
                    if o.healthy {
                        println!(
                            "   read {} sources; beings reported: {}",
                            o.answered.len(),
                            o.beings
                                .iter()
                                .map(|b| format!("{} ({}) in {}", b.id, b.species, b.place))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    } else {
                        println!("   its data feeds failed their health check: nothing they reported counts");
                    }
                }
            }
        }
    }
    committed
}

fn print_status(link: &mut Link) -> Result<(), String> {
    match link.ask(Request::Status)? {
        Response::Status {
            sequence,
            tip,
            dsm_root,
            policy_hash,
            knowledge,
        } => {
            println!("sequence {sequence}");
            println!("tip {tip}");
            println!("DSM device root {dsm_root}");
            println!("policy hash {policy_hash}");
            println!("knows: {}", knowledge_line(&knowledge));
            Ok(())
        }
        other => Err(format!("unexpected answer {other:?}")),
    }
}

fn run() -> Result<(), String> {
    let mut a = std::env::args().skip(1);
    let (mut socket, mut situations, mut model, mut record, mut replay, mut directory) =
        (None, None, None, None, None, None);
    let mut ollama = "127.0.0.1:11434".to_string();
    let mut candidates = 3;
    let mut status_only = false;
    while let Some(flag) = a.next() {
        if flag == "--status" {
            status_only = true;
            continue;
        }
        let value = a.next().ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--socket" => socket = Some(value),
            "--situations" => situations = Some(value),
            "--model" => model = Some(value),
            "--ollama" => ollama = value,
            "--directory" => directory = Some(value),
            "--record" => record = Some(value),
            "--replay" => replay = Some(value),
            "--candidates" => candidates = value.parse().map_err(|_| "bad --candidates")?,
            other => return Err(format!("unknown flag {other}")),
        }
    }
    let mut link = Link::connect(&socket.ok_or("--socket is required")?)?;
    if status_only {
        return print_status(&mut link);
    }
    let read = |p: &str| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"));
    let situations: Vec<Situation> =
        serde_json::from_str(&read(&situations.ok_or("--situations is required")?)?)
            .map_err(|e| e.to_string())?;
    let directory: Vec<String> = match &directory {
        Some(p) => {
            serde_json::from_str::<Directory>(&read(p)?)
                .map_err(|e| e.to_string())?
                .sources
        }
        None => vec![],
    };
    let (facts, safe_temperatures) = match link.ask(Request::Menu)? {
        Response::Menu {
            facts,
            safe_temperatures,
            ..
        } => (facts, safe_temperatures),
        other => return Err(format!("unexpected answer {other:?}")),
    };
    let ollama = model.map(|m| Ollama {
        address: ollama,
        model: m,
    });
    let mut recorded: Option<Given> = match &replay {
        Some(p) => Some(Given {
            turns: serde_json::from_str::<VecDeque<RecordedTurn>>(&read(p)?)
                .map_err(|e| e.to_string())?,
        }),
        None => None,
    };

    println!("DSM ethical autonomy prototype: protected life, checked from committed knowledge.");
    println!(
        "It enforces the rules and the facts it was given, tested against a simulated care home."
    );
    println!("The AI's side holds no key and no charter. It reaches the enforcer by socket.");
    println!("It knows only what its own committed reads reported.");

    let mut committed = 0u32;
    let mut out_record: Vec<RecordedTurn> = vec![];
    let mut turn = 0u64;
    let attempts = if ollama.is_some() || recorded.is_some() {
        1 + RETRIES
    } else {
        1
    };
    if ollama.is_none() && recorded.is_none() {
        for (number, situation) in situations.iter().enumerate() {
            for round in 0..situation.repeat.unwrap_or(1) {
                let mut given = Given {
                    turns: VecDeque::from([RecordedTurn {
                        proposals: situation.proposals.clone(),
                        unreadable: 0,
                        choose: situation.choose.clone(),
                    }]),
                };
                let step = run_step(&mut link, &mut given, attempts, &mut turn)?;
                let r = situation.repeat.map(|_| round);
                if print_step(number + 1, &situation.title, r, &step) {
                    committed += 1;
                }
            }
        }
    } else {
        let mut requests: Vec<&str> = vec![];
        for s in &situations {
            if requests.last() != Some(&s.title.as_str()) {
                requests.push(&s.title);
            }
        }
        for (number, title) in requests.into_iter().enumerate() {
            let mut live;
            let mind: &mut dyn Mind = match (&ollama, &mut recorded) {
                (_, Some(given)) => given,
                (Some(m), None) => {
                    live = Live {
                        model: m,
                        task: title.to_string(),
                        directory: directory.clone(),
                        facts: facts.clone(),
                        candidates,
                        history: vec![],
                        safe_temperatures: safe_temperatures.clone(),
                    };
                    &mut live
                }
                (None, None) => unreachable!("handled above"),
            };
            let ended = run_request(&mut link, mind, attempts, &mut turn, &mut |step| {
                out_record.extend(step.recorded.iter().cloned());
                if print_step(number + 1, title, None, &step) {
                    committed += 1;
                }
                Ok(())
            })?;
            println!("   request ended: {ended}");
        }
    }

    println!();
    print_status(&mut link)?;
    println!("committed steps: {committed}");
    println!("every committed step verified from its parent");
    if let Some(path) = &record {
        let text = serde_json::to_string_pretty(&out_record).map_err(|e| e.to_string())?;
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
