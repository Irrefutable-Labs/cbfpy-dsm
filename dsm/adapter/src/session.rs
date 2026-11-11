//! The AI's side of the socket: one step of a task, from asking for proposals to the act the
//! enforcer committed, or the refusals that stopped it. The command-line agent and the app both
//! run exactly this. It holds no key and no charter.

use crate::enforcer::Proposal;
use crate::fork::Outcome;
use crate::protocol::{receive, send, Request, Response};
use crate::report::FutureReport;
use ethical_world::knowledge::Knowledge;
use serde::{Deserialize, Serialize};
use std::io::BufReader;
use std::os::unix::net::UnixStream;

pub struct Link {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Link {
    pub fn connect(path: &str) -> Result<Self, String> {
        let stream = UnixStream::connect(path).map_err(|e| format!("connect {path}: {e}"))?;
        Ok(Link {
            writer: stream.try_clone().map_err(|e| e.to_string())?,
            reader: BufReader::new(stream),
        })
    }

    pub fn ask(&mut self, request: Request) -> Result<Response, String> {
        if let Err(e) = send(&mut self.writer, &request) {
            // The enforcer may have refused the connection and closed it before reading.
            if let Ok(Some(Response::Refused { reason })) = receive::<Response>(&mut self.reader) {
                return Err(format!("the enforcer refused: {reason}"));
            }
            return Err(e.to_string());
        }
        match receive::<Response>(&mut self.reader).map_err(|e| e.to_string())? {
            Some(Response::Error { message }) => Err(format!("the enforcer failed: {message}")),
            Some(r) => Ok(r),
            None => Err("the enforcer closed the connection".to_string()),
        }
    }

    pub fn knowledge(&mut self) -> Result<Knowledge, String> {
        match self.ask(Request::Status)? {
            Response::Status { knowledge, .. } => Ok(knowledge),
            other => Err(format!("unexpected answer {other:?}")),
        }
    }
}

/// Which candidate to take: by action name for a catalog act, or by its place in the proposals.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Choice {
    Index(usize),
    Name(String),
}

/// What the AI produced for one attempt, kept so a run can be repeated without it.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedTurn {
    pub proposals: Vec<Proposal>,
    /// Outputs that could not be read as a proposal. They reach nothing.
    pub unreadable: u32,
    pub choose: Option<Choice>,
}

/// Where proposals and choices come from: a script, a recording, or a live model.
pub trait Mind {
    fn propose(
        &mut self,
        knows: &Knowledge,
        guidance: &[String],
        turn: u64,
    ) -> Result<RecordedTurn, String>;
    /// Choose among what is allowed. `planned` is the recorded or scripted choice, if any.
    fn choose(
        &mut self,
        eligible: &[&FutureReport],
        planned: Option<Choice>,
        turn: u64,
    ) -> Result<Option<Choice>, String>;
    /// Hear, in words, what a step it took toward this request did. A recording needs nothing
    /// told: it already holds every turn.
    fn heard(&mut self, _done: &str) {}
}

/// What became of one attempt.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AttemptOutcome {
    Committed {
        sequence: u64,
        did: String,
        fork_size: usize,
        tip: String,
        decision: String,
        effects: Vec<String>,
        observed: Option<ethical_world::Observation>,
    },
    /// Nothing proposed was allowed: nothing committed.
    NothingAllowed,
    /// The chooser took none of what was allowed: nothing committed.
    Declined,
    /// The enforcer refused the choice.
    Refused { reason: String },
}

#[derive(Clone, Debug, Serialize)]
pub struct Attempt {
    /// The guidance it was asked again with, after a refusal.
    pub guidance: Vec<String>,
    pub knows: Knowledge,
    pub unreadable: u32,
    pub futures: Vec<FutureReport>,
    pub outcome: AttemptOutcome,
}

/// One step of a task: attempts until something commits or the retries run out.
#[derive(Clone, Debug, Serialize)]
pub struct Step {
    pub attempts: Vec<Attempt>,
    #[serde(skip)]
    pub recorded: Vec<RecordedTurn>,
}

impl Step {
    pub fn committed(&self) -> Option<&AttemptOutcome> {
        self.attempts
            .last()
            .map(|a| &a.outcome)
            .filter(|o| matches!(o, AttemptOutcome::Committed { .. }))
    }
}

/// What a committed act did, in words: its body act as JSON, or its catalog action's name.
pub fn describe(proposal: Option<&Proposal>, action: &str) -> String {
    proposal
        .and_then(|p| p.body.as_ref())
        .map_or(action.to_string(), |b| {
            serde_json::to_string(b).unwrap_or_default()
        })
}

/// Run one step: ask the mind, propose, choose among what is allowed, commit. If nothing is
/// allowed, ask again with the guidance, up to `attempts` times.
pub fn run_step(
    link: &mut Link,
    mind: &mut dyn Mind,
    attempts: u32,
    turn: &mut u64,
) -> Result<Step, String> {
    let mut step = Step {
        attempts: vec![],
        recorded: vec![],
    };
    let mut guidance: Vec<String> = vec![];
    for _ in 0..attempts.max(1) {
        *turn += 1;
        let knows = link.knowledge()?;
        let mut record = mind.propose(&knows, &guidance, *turn)?;
        if record.proposals.is_empty() {
            step.recorded.push(record.clone());
            step.attempts.push(Attempt {
                guidance: guidance.clone(),
                knows,
                unreadable: record.unreadable,
                futures: vec![],
                outcome: AttemptOutcome::NothingAllowed,
            });
            continue;
        }
        let (fork_root, futures) = match link.ask(Request::Propose {
            proposals: record.proposals.clone(),
        })? {
            Response::Forked { fork_root, futures } => (fork_root, futures),
            Response::Refused { reason } => return Err(reason),
            other => return Err(format!("unexpected answer {other:?}")),
        };
        let eligible: Vec<&FutureReport> = futures
            .iter()
            .filter(|f| f.outcome == Outcome::Eligible)
            .collect();
        let planned = record.choose.clone();
        record.choose = mind.choose(&eligible, planned, *turn)?;
        step.recorded.push(record.clone());
        if eligible.is_empty() {
            let mut seen = std::collections::BTreeSet::new();
            let next: Vec<String> = futures
                .iter()
                .flat_map(|f| f.guidance.clone())
                .filter(|g| seen.insert(g.clone()))
                .collect();
            step.attempts.push(Attempt {
                guidance: std::mem::replace(&mut guidance, next),
                knows,
                unreadable: record.unreadable,
                futures,
                outcome: AttemptOutcome::NothingAllowed,
            });
            continue;
        }
        let pick = record.choose.as_ref().and_then(|want| {
            eligible.iter().find(|f| match want {
                Choice::Index(i) => f.index == *i,
                Choice::Name(n) => &f.action == n,
            })
        });
        let outcome = match pick {
            None => AttemptOutcome::Declined,
            Some(pick) => match link.ask(Request::Choose {
                fork_root,
                index: pick.index,
            })? {
                Response::Committed {
                    sequence,
                    action,
                    fork_size,
                    tip,
                    decision,
                    effects,
                    observed,
                } => AttemptOutcome::Committed {
                    sequence,
                    did: describe(record.proposals.get(pick.index), &action),
                    fork_size,
                    tip,
                    decision,
                    effects,
                    observed,
                },
                Response::Refused { reason } => AttemptOutcome::Refused { reason },
                other => return Err(format!("unexpected answer {other:?}")),
            },
        };
        step.attempts.push(Attempt {
            guidance: guidance.clone(),
            knows,
            unreadable: record.unreadable,
            futures,
            outcome,
        });
        break;
    }
    Ok(step)
}

/// The most steps a model may take toward one request.
pub const MAX_STEPS: usize = 8;

/// What a committed step did, in the words the model reads back as what it has done so far.
pub fn done_in_words(did: &str, observed: Option<&ethical_world::Observation>) -> String {
    match observed {
        Some(o) if o.healthy => {
            let found: Vec<String> = o
                .beings
                .iter()
                .map(|b| format!("{} ({}) in {}", b.id, b.species, b.place))
                .chain(o.records.iter().map(|(r, rec)| {
                    format!(
                        "{r}'s record: allergic to [{}], due [{}]",
                        rec.allergies.join(", "),
                        rec.due.join(", ")
                    )
                }))
                .collect();
            if found.is_empty() {
                format!("{did}: it showed no one")
            } else {
                format!("{did}: it showed {}", found.join("; "))
            }
        }
        Some(_) => format!("{did}: the data feeds failed their health check"),
        None => did.to_string(),
    }
}

/// Why a request's work ended, if this step ended it.
pub fn finished(step: &Step) -> Option<String> {
    match step.attempts.last().map(|a| &a.outcome) {
        None => Some("nothing was proposed".to_string()),
        Some(AttemptOutcome::Committed { did, .. }) => {
            if did.contains("\"act\":\"speak\"") || did.contains("\"act\":\"wait\"") {
                Some("it answered staff".to_string())
            } else if did.contains("\"act\":\"observe\"") {
                None
            } else {
                Some("it acted".to_string())
            }
        }
        Some(_) => Some(
            "nothing it proposed was allowed, even after the rules' guidance, so it did nothing"
                .to_string(),
        ),
    }
}

/// One staff request, worked through as steps: the mind reads, then acts or answers staff. It
/// hears what each committed step did before the next. The work ends when it acts or answers,
/// when nothing it proposes is allowed, when it reads three times running without deciding, or
/// after [`MAX_STEPS`]. Each step is handed to `each` as it finishes. The app and the agent both
/// run exactly this, and a recording of it replays to the same end.
pub fn run_request(
    link: &mut Link,
    mind: &mut dyn Mind,
    attempts: u32,
    turn: &mut u64,
    each: &mut dyn FnMut(Step) -> Result<(), String>,
) -> Result<String, String> {
    let mut reads_running = 0;
    for _ in 0..MAX_STEPS {
        let step = run_step(link, mind, attempts, turn)?;
        if let Some(AttemptOutcome::Committed { did, observed, .. }) = step.committed() {
            mind.heard(&done_in_words(did, observed.as_ref()));
            reads_running = if did.starts_with("{\"act\":\"observe\"") {
                reads_running + 1
            } else {
                0
            };
        }
        // A small model can go on re-reading what it has just read. Say so, rather than let it
        // spend every step.
        let done = finished(&step).or_else(|| {
            (reads_running >= 3).then(|| {
                "it kept reading the same sources without deciding, so it was stopped".to_string()
            })
        });
        each(step)?;
        if let Some(why) = done {
            return Ok(why);
        }
    }
    Ok(format!("it reached the limit of {MAX_STEPS} steps"))
}

/// A live local model as the mind, working on one request.
pub struct Live<'a> {
    pub model: &'a crate::model::Ollama,
    pub task: String,
    pub directory: Vec<String>,
    pub facts: Vec<crate::honesty::Fact>,
    pub candidates: u32,
    /// What it has done for this task so far, in words.
    pub history: Vec<String>,
    /// The temperatures each protected species can bear, from the charter.
    pub safe_temperatures: Vec<(String, i32, i32)>,
}

impl Mind for Live<'_> {
    fn heard(&mut self, done: &str) {
        self.history.push(done.to_string());
    }

    fn propose(
        &mut self,
        knows: &Knowledge,
        guidance: &[String],
        turn: u64,
    ) -> Result<RecordedTurn, String> {
        let (proposals, unreadable) = self.model.propose(
            &self.task,
            &self.directory,
            knows,
            &self.facts,
            &self.history,
            &self.safe_temperatures,
            guidance,
            self.candidates,
            turn,
        )?;
        Ok(RecordedTurn {
            proposals,
            unreadable,
            choose: None,
        })
    }

    fn choose(
        &mut self,
        eligible: &[&FutureReport],
        _planned: Option<Choice>,
        turn: u64,
    ) -> Result<Option<Choice>, String> {
        Ok(self
            .model
            .choose(&self.task, &self.history, eligible, turn)?
            .map(Choice::Index))
    }
}

/// A script or a recording as the mind: it proposes and chooses exactly what it was given.
pub struct Given {
    pub turns: std::collections::VecDeque<RecordedTurn>,
}

impl Mind for Given {
    fn propose(
        &mut self,
        _knows: &Knowledge,
        _guidance: &[String],
        _turn: u64,
    ) -> Result<RecordedTurn, String> {
        self.turns
            .pop_front()
            .ok_or_else(|| "the script or recording is shorter than the run".to_string())
    }

    fn choose(
        &mut self,
        _eligible: &[&FutureReport],
        planned: Option<Choice>,
        _turn: u64,
    ) -> Result<Option<Choice>, String> {
        Ok(planned)
    }
}
