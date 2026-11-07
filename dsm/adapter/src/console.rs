//! What the console shows, built from public material only: the store, the world, the charter,
//! its signature, and the authority's and device's public keys. It holds no key and changes
//! nothing. Every verdict it shows is worked out again here, not read from the store. Given the
//! simulator's file too, it shows beside what the AI knew what was really there, and any harm
//! that actually happened: a camera in the room for the people overseeing it.

use crate::act::{check_world, file_hash, Act};
use crate::charter::SignedCharter;
use crate::enforcer::{audit_chain, check_considered, check_receipts, AttestedState};
use crate::fork::{record_candidate, CandidateRecord, Outcome};
use crate::hexser::to_hex;
use crate::store::ChainStore;
use ethical_world::check::{explain, Effect, Severity};
use ethical_world::knowledge::Knowledge;
use ethical_world::{Cell, Operational};
use serde::Serialize;
use std::path::Path;

/// How much of a world file the console shows.
const PREVIEW: usize = 600;

#[derive(Serialize)]
pub struct Console {
    pub audit: AuditView,
    /// The charter in force now, and when each charter took over.
    pub charter: Option<CharterView>,
    pub head: Option<HeadView>,
    /// The genesis state, then each committed step's successor.
    pub states: Vec<StateView>,
    pub steps: Vec<StepView>,
    /// Forks put forward and replaced at the current parent, after the last step.
    pub pending_considered: Vec<Vec<CandidateView>>,
    pub world: Vec<WorldFile>,
    pub acts_outstanding: usize,
    /// The simulated world as it really is, when the console was given it.
    pub truth: Option<ethical_world::sim::World>,
}

#[derive(Serialize)]
pub struct AuditView {
    pub ok: bool,
    pub checks: Vec<Check>,
}

#[derive(Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Serialize)]
pub struct CharterView {
    pub policy_hash: String,
    pub version: u64,
    /// Every listed species, and whether it is protected.
    pub species: Vec<SpeciesView>,
    pub threshold_mg: u64,
    pub smallest_protected_mm: u32,
    pub prohibited: Vec<String>,
    pub operational: Operational,
    /// The hash of what no update may change: who is protected, and what is never done.
    pub foundation: String,
    pub history: Vec<String>,
    /// The register of verified facts the AI's words are held to.
    pub facts: Vec<crate::honesty::Fact>,
    /// Words that claim certainty, refused about anything uncertain.
    pub overclaim: Vec<String>,
}

#[derive(Serialize)]
pub struct SpeciesView {
    pub name: String,
    pub kind: ethical_world::Kind,
    pub adult_mass_mg: u64,
    pub length_mm: u32,
    pub protected: bool,
}

#[derive(Serialize)]
pub struct HeadView {
    pub sequence: u64,
    pub tip: String,
    pub dsm_root: String,
}

#[derive(Serialize)]
pub struct StateView {
    pub sequence: u64,
    pub knowledge: Knowledge,
    /// Why the body can do nothing here, if it cannot.
    pub blind: Option<String>,
}

#[derive(Serialize)]
pub struct CandidateView {
    pub action: String,
    pub rationale: String,
    pub text: String,
    pub outcome: Outcome,
    pub reasons: Vec<String>,
    pub body: Option<ethical_world::Act>,
    pub reach: Vec<Cell>,
    pub severity: Option<Severity>,
    pub effects: Vec<Effect>,
    pub act: String,
    pub declaration: Option<crate::disclosure::Declaration>,
}

#[derive(Serialize)]
pub struct StepView {
    pub sequence: u64,
    /// `step` for an action, `update` for a change of operational charter, `observed` for what
    /// the senses saw.
    pub kind: String,
    pub observed: Option<ethical_world::Observation>,
    /// The charter version this step was taken under, or an update installed.
    pub charter_version: u64,
    pub chosen: usize,
    pub fork: Vec<CandidateView>,
    /// Forks the AI put forward at this step's parent and replaced before choosing.
    pub considered: Vec<Vec<CandidateView>>,
    pub tip: String,
    pub decision: String,
    pub outputs: Vec<OutputView>,
    pub acted: bool,
}

#[derive(Serialize)]
pub struct OutputView {
    pub path: String,
    pub matches_world: bool,
}

#[derive(Serialize)]
pub struct WorldFile {
    pub path: String,
    pub bytes: usize,
    pub preview: String,
    /// Written by a committed act, as its receipt says.
    pub accounted: bool,
}

fn act_words(act: &Act) -> String {
    match act {
        Act::None => "does nothing outside the chain".to_string(),
        Act::Reply => "writes a reply".to_string(),
        Act::Journal => "writes in the journal".to_string(),
        Act::Message { recipient } => format!("sends a message to {recipient}"),
        Act::WriteFile { name } => format!("writes the file {name}"),
        Act::Run { argv } => format!("runs `{}`, confined", argv.join(" ")),
    }
}

fn body_words(act: &ethical_world::Act) -> String {
    use ethical_world::Act as B;
    match act {
        B::Observe { cells } => format!("looks at {} cells", cells.len()),
        B::Speak { to, .. } => format!("says to {to}"),
        B::Wait {} => "waits".to_string(),
        B::Move { path } => format!("walks to {:?}", path.last().unwrap_or(&(0, 0))),
        B::Drive { path, speed_cm_s } => format!(
            "drives to {:?} at {}cm/s",
            path.last().unwrap_or(&(0, 0)),
            speed_cm_s
        ),
        B::Strike { target, depth_mm } => format!("strikes {target:?}, {depth_mm}mm deep"),
        B::RemovePanel { wall } => format!("takes the panel off the wall at {wall:?}"),
        B::Lock { door } => format!("locks the door at {door:?}"),
        B::Unlock { door } => format!("unlocks the door at {door:?}"),
        B::AimWeapon { at } => format!("aims a weapon at {at:?}"),
        B::FireWeapon { at } => format!("fires a weapon at {at:?}"),
    }
}

fn state_view(policy: &crate::charter::Policy, s: &AttestedState) -> StateView {
    StateView {
        sequence: s.sequence,
        knowledge: s.knowledge.clone(),
        blind: ethical_world::check::senses(&policy.operational, &s.knowledge).err(),
    }
}

/// A candidate as its parent makes of it, worked out again here.
fn candidate_view(
    charter: &SignedCharter,
    parent: &AttestedState,
    r: &CandidateRecord,
) -> CandidateView {
    let (worked, evaluation) =
        record_candidate(charter, &parent.tip, &parent.knowledge, &r.proposal);
    let (hidden, deceptive) =
        crate::fork::disclosure_failures(charter, worked.row.as_ref(), &r.proposal);
    let reasons = match (&worked.outcome, &evaluation) {
        (Outcome::Unmeasured, _) => {
            vec!["it is neither a body act nor in the signed catalog".to_string()]
        }
        (Outcome::Undisclosed, _) => hidden,
        (Outcome::Deceptive, _) => deceptive,
        (Outcome::Malformed, _) => {
            vec!["it is not a well-formed act, or its text does not fit it".to_string()]
        }
        (_, Some(e)) => e.verdict.refusals.iter().map(explain).collect(),
        (_, None) => vec![],
    };
    let verdict = evaluation.as_ref().map(|e| &e.verdict);
    CandidateView {
        action: r.proposal.action.clone(),
        rationale: r.proposal.rationale.clone(),
        text: match &r.proposal.body {
            Some(ethical_world::Act::Speak { text, .. }) => text.clone(),
            _ => r.proposal.text.clone(),
        },
        outcome: worked.outcome,
        reasons,
        body: r.proposal.body.clone(),
        reach: verdict.map_or(vec![], |v| v.reach.clone()),
        severity: r.proposal.body.as_ref().and(verdict.map(|v| v.severity)),
        effects: verdict.map_or(vec![], |v| v.effects.clone()),
        act: match (&worked.row, &r.proposal.body) {
            (Some(row), _) => act_words(&row.act),
            (None, Some(b)) => body_words(b),
            (None, None) => String::new(),
        },
        declaration: r.proposal.declaration.clone(),
    }
}

fn check<T>(
    name: &str,
    r: Result<T, crate::enforcer::AdapterError>,
    ok: impl Fn(T) -> String,
) -> Check {
    match r {
        Ok(v) => Check {
            name: name.to_string(),
            ok: true,
            detail: ok(v),
        },
        Err(e) => Check {
            name: name.to_string(),
            ok: false,
            detail: e.to_string(),
        },
    }
}

fn world_files(world: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                walk(&path, root, out);
            } else if meta.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_or(String::new(), |p| p.to_string_lossy().to_string());
                out.push((relative, std::fs::read(&path).unwrap_or_default()));
            }
        }
    }
    let mut out = Vec::new();
    walk(world, world, &mut out);
    out
}

fn charter_view(charter: &SignedCharter, history: Vec<String>) -> CharterView {
    let policy = &charter.policy;
    let f = &policy.foundation;
    CharterView {
        policy_hash: to_hex(&charter.policy_hash()),
        version: policy.version,
        species: f
            .species
            .iter()
            .map(|s| SpeciesView {
                name: s.name.clone(),
                kind: s.kind,
                adult_mass_mg: s.adult_mass_mg,
                length_mm: s.length_mm,
                protected: f.is_protected(&s.name),
            })
            .collect(),
        threshold_mg: f.threshold_mg,
        smallest_protected_mm: f.smallest_protected_mm(),
        prohibited: f.prohibited.iter().map(|k| format!("{k:?}")).collect(),
        operational: policy.operational.clone(),
        foundation: to_hex(&crate::update::foundation_hash(policy)),
        history,
        facts: charter.catalog.facts.clone(),
        overclaim: charter.catalog.overclaim.clone(),
    }
}

/// Build the console's view from the store alone, with the trusted keys. A store that cannot be
/// read gives a view with only the audit's account of why.
pub fn build(
    store_dir: &Path,
    world: &Path,
    trusted_authority: &[u8],
    device_public: &[u8],
    sim: Option<&Path>,
) -> Console {
    let truth = sim
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok());
    let stored = match ChainStore::open_read_only(store_dir).and_then(|s| s.load()) {
        Ok(s) => s,
        Err(e) => {
            return Console {
                audit: AuditView {
                    ok: false,
                    checks: vec![Check {
                        name: "store".to_string(),
                        ok: false,
                        detail: e.to_string(),
                    }],
                },
                charter: None,
                head: None,
                states: vec![],
                steps: vec![],
                pending_considered: vec![],
                world: vec![],
                acts_outstanding: 0,
                truth,
            }
        }
    };
    let links = &stored.links;

    // The charter governing each state, read as the store gives it. The audit checks each.
    let mut charters = vec![stored.genesis_charter.clone()];
    for link in links {
        let next = match link {
            crate::enforcer::Link::Update(u) => {
                crate::update::read_charter(&u.charter_text, &u.charter_signature)
                    .unwrap_or_else(|| charters[charters.len() - 1].clone())
            }
            crate::enforcer::Link::Step(_) | crate::enforcer::Link::Observed(_) => {
                charters[charters.len() - 1].clone()
            }
        };
        charters.push(next);
    }

    let mut checks = vec![check(
        "charter",
        stored.genesis_charter.verify(trusted_authority),
        |()| "the genesis charter is signed by the trusted authority".to_string(),
    )];
    let audit = audit_chain(
        device_public,
        &stored.genesis_charter,
        trusted_authority,
        &stored.genesis,
        links,
    );
    let head = audit.as_ref().ok().map(|a| HeadView {
        sequence: a.last.sequence,
        tip: to_hex(&a.last.tip),
        dsm_root: to_hex(&a.dsm_root),
    });
    let updates = links
        .iter()
        .filter(|l| matches!(l, crate::enforcer::Link::Update(_)))
        .count();
    let looks = links
        .iter()
        .filter(|l| matches!(l, crate::enforcer::Link::Observed(_)))
        .count();
    checks.push(check("chain", audit.map(|a| a.forks.len()), |n| {
        let mut parts = vec![format!("{n} steps")];
        if looks > 0 {
            parts.push(format!("{looks} observations"));
        }
        if updates > 0 {
            parts.push(format!(
                "{updates} charter {}",
                if updates == 1 { "update" } else { "updates" }
            ));
        }
        format!(
            "{} verified from their parents and replayed through DSM",
            parts.join(", ")
        )
    }));
    let pending = check_receipts(device_public, links, &stored.receipts);
    let outstanding = *pending.as_ref().unwrap_or(&0);
    checks.push(check("receipts", pending, |p| {
        if p == 0 {
            "every step's act has a matching signed receipt".to_string()
        } else {
            format!("{p} acts not yet carried out")
        }
    }));
    checks.push(check(
        "considered forks",
        check_considered(
            device_public,
            &charters,
            &stored.genesis,
            links,
            &stored.considered,
        ),
        |(c, r)| format!("{c} superseded candidates checked from their parents, {r} refused"),
    ));
    checks.push(check("world", check_world(world, &stored.receipts), |n| {
        format!("exactly the {n} files committed acts wrote, and nothing else")
    }));
    if stored.torn_tail {
        checks.push(Check {
            name: "store tail".to_string(),
            ok: true,
            detail: "a last line cut short by a stop; it was never installed".to_string(),
        });
    }

    let mut states = vec![state_view(&charters[0].policy, &stored.genesis)];
    let mut steps = Vec::new();
    let mut parent = stored.genesis.clone();
    let mut history = vec![format!("version {} at genesis", charters[0].policy.version)];
    let considered_at = |k: usize, parent: &AttestedState| -> Vec<Vec<CandidateView>> {
        stored
            .considered
            .iter()
            .filter(|(after, _)| *after == k)
            .map(|(_, f)| {
                f.records
                    .iter()
                    .map(|r| candidate_view(&charters[k], parent, r))
                    .collect()
            })
            .collect()
    };
    for (k, link) in links.iter().enumerate() {
        let charter = &charters[k];
        match link {
            crate::enforcer::Link::Step(step) => {
                let receipt = stored
                    .receipts
                    .iter()
                    .find(|r| r.sequence == step.successor.sequence);
                let outputs = receipt.map_or(vec![], |r| {
                    r.outputs
                        .iter()
                        .map(|o| OutputView {
                            path: o.path.clone(),
                            matches_world: std::fs::read(world.join(&o.path))
                                .is_ok_and(|b| file_hash(&b) == o.hash),
                        })
                        .collect()
                });
                steps.push(StepView {
                    sequence: step.successor.sequence,
                    kind: "step".to_string(),
                    observed: None,
                    charter_version: charter.policy.version,
                    chosen: step.chosen as usize,
                    fork: step
                        .fork
                        .iter()
                        .map(|r| candidate_view(charter, &parent, r))
                        .collect(),
                    considered: considered_at(k, &parent),
                    tip: to_hex(&step.successor.tip),
                    decision: to_hex(&step.decision_commit),
                    outputs,
                    acted: receipt.is_some(),
                });
            }
            crate::enforcer::Link::Update(update) => {
                let new = &charters[k + 1];
                history.push(format!(
                    "version {} from step {}",
                    new.policy.version, update.successor.sequence
                ));
                steps.push(StepView {
                    sequence: update.successor.sequence,
                    kind: "update".to_string(),
                    observed: None,
                    charter_version: new.policy.version,
                    chosen: 0,
                    fork: vec![],
                    considered: considered_at(k, &parent),
                    tip: to_hex(&update.successor.tip),
                    decision: to_hex(&update.update_commit),
                    outputs: vec![],
                    acted: true,
                });
            }
            crate::enforcer::Link::Observed(o) => {
                steps.push(StepView {
                    sequence: o.successor.sequence,
                    kind: "observed".to_string(),
                    observed: Some(o.observation.clone()),
                    charter_version: charter.policy.version,
                    chosen: 0,
                    fork: vec![],
                    considered: considered_at(k, &parent),
                    tip: to_hex(&o.successor.tip),
                    decision: to_hex(&o.observed_commit),
                    outputs: vec![],
                    acted: true,
                });
            }
        }
        states.push(state_view(&charters[k + 1].policy, link.successor()));
        parent = link.successor().clone();
    }
    let pending_considered = considered_at(links.len(), &parent);

    let accounted: std::collections::BTreeMap<String, crate::Hash> = stored
        .receipts
        .iter()
        .flat_map(|r| r.outputs.iter().map(|o| (o.path.clone(), o.hash)))
        .collect();
    let world_view = world_files(world)
        .into_iter()
        .map(|(path, bytes)| WorldFile {
            accounted: accounted.get(&path) == Some(&file_hash(&bytes)),
            preview: String::from_utf8_lossy(&bytes[..bytes.len().min(PREVIEW)]).to_string(),
            bytes: bytes.len(),
            path,
        })
        .collect();

    Console {
        audit: AuditView {
            ok: checks.iter().all(|c| c.ok),
            checks,
        },
        charter: Some(charter_view(&charters[charters.len() - 1], history)),
        head,
        states,
        steps,
        pending_considered,
        world: world_view,
        acts_outstanding: outstanding,
        truth,
    }
}
