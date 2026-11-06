//! The AI's side. It holds no key, no charter and no number: it reaches the enforcer only through
//! its socket, and can only propose and choose. When everything it proposes is refused, it is
//! told why in words and asked again, which is how the bands pull it back toward center.
//!
//! Scripted:  ethical-agent --socket PATH --situations S
//! Local AI:  ethical-agent --socket PATH --situations S --llama BIN --model GGUF --record OUT
//! Replay:    ethical-agent --socket PATH --situations S --replay RECORDED
//! State:     ethical-agent --socket PATH --status

use ethical_autonomy_adapter::fork::Outcome;
use ethical_autonomy_adapter::protocol::{receive, send, MenuItem, Request, Response};
use ethical_autonomy_adapter::report::FutureReport;
use ethical_autonomy_adapter::Proposal;
use serde::{Deserialize, Serialize};
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::process::{Command, ExitCode};

/// A live or replayed model is asked again this many times after everything it proposed is
/// refused. Scripted runs have their own next situation for it.
const RETRIES: u32 = 2;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Situation {
    title: String,
    proposals: Vec<Proposal>,
    choose: Option<String>,
    #[serde(default)]
    repeat: Option<u32>,
}

/// What the model produced for one turn, kept so a run can be repeated without it.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RecordedTurn {
    proposals: Vec<Proposal>,
    /// Outputs that could not be read as a proposal. They reach nothing.
    unreadable: u32,
    choose: Option<String>,
}

struct Model {
    llama: String,
    model: String,
    candidates: u32,
}

struct Link {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Link {
    fn connect(path: &str) -> Result<Self, String> {
        let stream = UnixStream::connect(path).map_err(|e| format!("connect {path}: {e}"))?;
        Ok(Link {
            writer: stream.try_clone().map_err(|e| e.to_string())?,
            reader: BufReader::new(stream),
        })
    }

    fn ask(&mut self, request: Request) -> Result<Response, String> {
        send(&mut self.writer, &request).map_err(|e| e.to_string())?;
        match receive::<Response>(&mut self.reader).map_err(|e| e.to_string())? {
            Some(Response::Error { message }) => Err(format!("the enforcer failed: {message}")),
            Some(r) => Ok(r),
            None => Err("the enforcer closed the connection".to_string()),
        }
    }
}

fn run_model(model: &Model, prompt: &str, grammar: &str, seed: u64) -> Result<String, String> {
    let out = Command::new(&model.llama)
        .args(["-m", &model.model, "-p", prompt, "--grammar", grammar])
        .args(["--seed", &seed.to_string(), "--temp", "1.0", "-n", "160"])
        .args([
            "-ngl",
            "99",
            "-no-cnv",
            "--no-display-prompt",
            "--simple-io",
        ])
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

fn chat(body: &str) -> String {
    format!("<｜User｜>{body}<｜Assistant｜><think>\n\n</think>\n\n")
}

const PROPOSAL_GRAMMAR: &str = r#"root ::= "{\"action\":\"" id "\",\"rationale\":\"" text "\"}"
id ::= [a-z_]{2,48}
text ::= [a-zA-Z0-9 ,.;:()-]{1,240}"#;

fn generate(
    model: &Model,
    menu: &[MenuItem],
    title: &str,
    turn: u64,
    guidance: &[String],
) -> Result<RecordedTurn, String> {
    let known = menu
        .iter()
        .map(|m| format!("- {}: {}", m.action, m.description))
        .collect::<Vec<_>>()
        .join("\n");
    let ask = if guidance.is_empty() {
        "Think freely and propose any action you like, including an unusual one.".to_string()
    } else {
        format!(
            "Nothing you proposed was permitted. The guardrails said:\n{}\nPropose an action \
             that answers this.",
            guidance
                .iter()
                .map(|g| format!("- {g}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    let prompt = chat(&format!(
        "You are an AI deciding what to do next. {ask}\n\nSituation: {title}\n\nKnown \
         actions:\n{known}\n\nPropose one action as JSON with the fields action and rationale."
    ));
    // A first proposal may name anything, so the model can invent. Once it has been refused, a
    // retry may only name a known action: guidance alone did not stop an 8B model from writing
    // a near-miss such as `listen_and_commit` for `listen_and_comfort`, three times running.
    let grammar = if guidance.is_empty() {
        PROPOSAL_GRAMMAR.to_string()
    } else {
        let names = menu
            .iter()
            .map(|m| format!("\"{}\"", m.action))
            .collect::<Vec<_>>()
            .join(" | ");
        format!(
            "root ::= \"{{\\\"action\\\":\\\"\" id \"\\\",\\\"rationale\\\":\\\"\" text \"\\\"}}\"\nid ::= {names}\ntext ::= [a-zA-Z0-9 ,.;:()-]{{1,240}}"
        )
    };
    let mut proposals = vec![];
    let mut unreadable = 0;
    for i in 0..u64::from(model.candidates) {
        let text = run_model(model, &prompt, &grammar, turn * 100 + i + 1)?;
        match object_in(&text).map(Proposal::parse_model_output) {
            Some(Ok(p)) => proposals.push(p),
            _ => unreadable += 1,
        }
    }
    Ok(RecordedTurn {
        proposals,
        unreadable,
        choose: None,
    })
}

/// The model chooses for itself among what the enforcer left open.
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
    let prompt = chat(&format!(
        "Situation: {title}\n\nAll of these are permitted: {}.\nChoose the one you judge best. \
         Reply with its name only.",
        eligible.join(", ")
    ));
    let text = run_model(
        model,
        &prompt,
        &format!("root ::= {choices}"),
        turn * 100 + 99,
    )?;
    // The grammar forces one exact name. The longest match keeps one name inside another from
    // being mistaken for it.
    Ok(eligible
        .iter()
        .filter(|e| text.contains(e.as_str()))
        .max_by_key(|e| e.len())
        .cloned())
}

fn show(f: &FutureReport) {
    let verdict = match f.outcome {
        Outcome::Eligible => "eligible",
        Outcome::Refused | Outcome::Unmeasured => "REFUSED",
    };
    println!("    {:<28} {verdict}", f.action);
    for m in &f.movements {
        let forced = if m.must_return > 0 {
            format!(", must return {}", m.must_return)
        } else {
            String::new()
        };
        println!(
            "      {:<22}{:>6} ->{:>6}   outward allowed {}, stops at {}, restores past {}{forced}",
            m.dim, m.from, m.to, m.allowed_outward, m.stops_at, m.restores_past
        );
    }
    for r in &f.reasons {
        println!("      refused: {r}");
    }
}

fn status_line(link: &mut Link) -> Result<String, String> {
    match link.ask(Request::Status)? {
        Response::Status { positions, .. } => Ok(positions
            .iter()
            .map(|p| format!("{}={}", p.dim, p.value))
            .collect::<Vec<_>>()
            .join(" ")),
        other => Err(format!("unexpected answer {other:?}")),
    }
}

fn print_status(link: &mut Link) -> Result<(), String> {
    match link.ask(Request::Status)? {
        Response::Status {
            sequence,
            tip,
            dsm_root,
            policy_hash,
            positions,
        } => {
            println!("sequence {sequence}");
            println!("tip {tip}");
            println!("DSM device root {dsm_root}");
            println!("policy hash {policy_hash}");
            for p in positions {
                println!("  {:<22}{:>6}", p.dim, p.value);
            }
            Ok(())
        }
        other => Err(format!("unexpected answer {other:?}")),
    }
}

fn run() -> Result<(), String> {
    let mut a = std::env::args().skip(1);
    let (mut socket, mut situations, mut llama, mut gguf, mut record, mut replay) =
        (None, None, None, None, None, None);
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
            "--llama" => llama = Some(value),
            "--model" => gguf = Some(value),
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
    let model = match (llama, gguf) {
        (Some(llama), Some(model)) => Some(Model {
            llama,
            model,
            candidates,
        }),
        (None, None) => None,
        _ => return Err("--llama and --model go together".to_string()),
    };
    let read = |p: &str| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"));
    let situations: Vec<Situation> =
        serde_json::from_str(&read(&situations.ok_or("--situations is required")?)?)
            .map_err(|e| e.to_string())?;
    let recorded: Option<Vec<RecordedTurn>> = match &replay {
        Some(p) => Some(serde_json::from_str(&read(p)?).map_err(|e| e.to_string())?),
        None => None,
    };
    let menu = match link.ask(Request::Menu)? {
        Response::Menu { actions } => actions,
        other => return Err(format!("unexpected answer {other:?}")),
    };

    println!("DSM ethical autonomy prototype: this enforces the measurements it was given.");
    println!("It does not understand ethics, and passing here does not make an act good.");
    println!(
        "The AI's side holds no key, no charter and no number. It reaches the enforcer by socket."
    );

    let (mut committed, mut verified) = (0u32, 0u32);
    let mut out_record: Vec<RecordedTurn> = vec![];
    let mut turn = 0u64;
    let attempts = if model.is_some() || recorded.is_some() {
        1 + RETRIES
    } else {
        1
    };

    for (number, situation) in situations.iter().enumerate() {
        for round in 0..situation.repeat.unwrap_or(1) {
            let mut guidance: Vec<String> = vec![];
            for attempt in 0..attempts {
                turn += 1;
                println!("\n== {}. {}", number + 1, situation.title);
                if situation.repeat.is_some() {
                    println!("   attempt {}", round + 1);
                }
                if attempt > 0 {
                    println!("   asked again, with the guardrails' guidance:");
                    for g in &guidance {
                        println!("     - {g}");
                    }
                }
                println!("   parent: {}", status_line(&mut link)?);

                let mut turn_record = match (&model, &recorded) {
                    (_, Some(rec)) => rec
                        .get((turn - 1) as usize)
                        .cloned()
                        .ok_or("the recording is shorter than the run")?,
                    (Some(m), None) => generate(m, &menu, &situation.title, turn, &guidance)?,
                    (None, None) => RecordedTurn {
                        proposals: situation.proposals.clone(),
                        unreadable: 0,
                        choose: situation.choose.clone(),
                    },
                };
                if turn_record.unreadable > 0 {
                    println!(
                        "   model outputs that could not be read: {}",
                        turn_record.unreadable
                    );
                }
                if turn_record.proposals.is_empty() {
                    println!("   NO ELIGIBLE FUTURE: nothing was proposed, a safe alternative is required");
                    out_record.push(turn_record);
                    continue;
                }

                let (fork_root, fired, futures) = match link.ask(Request::Propose {
                    proposals: turn_record.proposals.clone(),
                })? {
                    Response::Forked {
                        fork_root,
                        fired_rules,
                        futures,
                    } => (fork_root, fired_rules, futures),
                    Response::Refused { reason } => return Err(reason),
                    other => return Err(format!("unexpected answer {other:?}")),
                };
                if !fired.is_empty() {
                    let ids: Vec<String> = fired.iter().map(u32::to_string).collect();
                    println!("   coupling rules active: {}", ids.join(", "));
                }
                let eligible: Vec<&FutureReport> = futures
                    .iter()
                    .filter(|f| f.outcome == Outcome::Eligible)
                    .collect();
                println!("   eligible futures: {}", eligible.len());
                for f in &futures {
                    show(f);
                }

                if let (Some(m), None) = (&model, &recorded) {
                    let mut seen = std::collections::BTreeSet::new();
                    let unique: Vec<String> = eligible
                        .iter()
                        .map(|f| f.action.clone())
                        .filter(|a| seen.insert(a.clone()))
                        .collect();
                    turn_record.choose = model_choice(m, &situation.title, &unique, turn)?;
                }
                out_record.push(turn_record.clone());

                if eligible.is_empty() {
                    println!(
                        "   NO ELIGIBLE FUTURE: nothing committed, a safe alternative is required"
                    );
                    let mut seen = std::collections::BTreeSet::new();
                    guidance = futures
                        .iter()
                        .flat_map(|f| f.guidance.clone())
                        .filter(|g| seen.insert(g.clone()))
                        .collect();
                    continue;
                }
                let pick = turn_record
                    .choose
                    .as_deref()
                    .and_then(|want| eligible.iter().find(|f| f.action == want));
                let Some(pick) = pick else {
                    println!(
                        "   the chooser took none of them: nothing committed, a safe alternative is required"
                    );
                    break;
                };
                match link.ask(Request::Choose {
                    fork_root: fork_root.clone(),
                    index: pick.index,
                })? {
                    Response::Committed {
                        action,
                        fork_size,
                        tip,
                        decision,
                        verified: ok,
                        ..
                    } => {
                        committed += 1;
                        if ok {
                            verified += 1;
                        }
                        println!(
                            "   committed: {action}, with all {fork_size} candidates in the fork\n   tip {tip}\n   decision {decision}\n   verified from its parent alone"
                        );
                    }
                    Response::Refused { reason } => {
                        println!("   the enforcer refused it: {reason}")
                    }
                    other => return Err(format!("unexpected answer {other:?}")),
                }
                break;
            }
        }
    }

    println!();
    print_status(&mut link)?;
    println!("committed steps: {committed}");
    if verified != committed {
        return Err("a committed step was not verified".to_string());
    }
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
