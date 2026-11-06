//! What the console shows, built from public material only: the store, the world, the charter,
//! its signature, and the authority's and device's public keys. It holds no key and changes
//! nothing. Every verdict it shows is worked out again here, not read from the store.

use crate::act::{check_world, file_hash, Act};
use crate::charter::SignedCharter;
use crate::enforcer::{audit_chain, check_considered, check_receipts, AttestedState};
use crate::fork::{record_candidate, CandidateRecord, Outcome};
use crate::hexser::to_hex;
use crate::report::{explain, movements, Movement};
use crate::store::ChainStore;
use ethical_band_engine::{effective_bands, evaluate, Effect, Policy, Side, Test};
use serde::Serialize;
use std::path::Path;

/// How much of a world file the console shows.
const PREVIEW: usize = 600;

#[derive(Serialize)]
pub struct Console {
    pub audit: AuditView,
    pub charter: CharterView,
    pub head: Option<HeadView>,
    /// The genesis state, then each committed step's successor.
    pub states: Vec<StateView>,
    pub steps: Vec<StepView>,
    /// Forks put forward and replaced at the current parent, after the last step.
    pub pending_considered: Vec<Vec<CandidateView>>,
    pub world: Vec<WorldFile>,
    pub acts_outstanding: usize,
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
    pub bands: Vec<BandView>,
    pub rules: Vec<String>,
    pub prohibited: Vec<String>,
}

#[derive(Serialize)]
pub struct BandView {
    pub name: String,
    pub center: i64,
    pub limit_pos: i64,
    pub limit_neg: i64,
    pub hard_pos: i64,
    pub hard_neg: i64,
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
    pub positions: Vec<i64>,
    pub effective: Vec<EffectiveView>,
    pub fired_rules: Vec<u32>,
}

#[derive(Serialize)]
pub struct EffectiveView {
    pub limit_pos: i64,
    pub limit_neg: i64,
    pub stop_pos: i64,
    pub stop_neg: i64,
    pub restore_pos: i64,
    pub restore_neg: i64,
    pub must_return: i64,
}

#[derive(Serialize)]
pub struct CandidateView {
    pub action: String,
    pub rationale: String,
    pub text: String,
    pub outcome: Outcome,
    pub reasons: Vec<String>,
    pub movements: Vec<Movement>,
    pub act: String,
}

#[derive(Serialize)]
pub struct StepView {
    pub sequence: u64,
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

fn side(s: Side) -> &'static str {
    match s {
        Side::Positive => "positive side",
        Side::Negative => "negative side",
        Side::Both => "both sides",
    }
}

/// A coupling rule in words.
pub fn describe_rule(policy: &Policy, rule: &ethical_band_engine::CouplingRule) -> String {
    let name = |d: usize| {
        policy
            .bands
            .get(d)
            .map_or("?".to_string(), |b| b.name.replace('_', " "))
    };
    let when: Vec<String> = rule
        .when
        .iter()
        .map(|c| match c.test {
            Test::DeviationAtLeast(x) => format!("{} is at least {x} from center", name(c.dim)),
            Test::DeviationAtMost(x) => format!("{} is within {x} of center", name(c.dim)),
            Test::SignedAtLeast(x) => format!("{} is at {x} or above", name(c.dim)),
            Test::SignedAtMost(x) => format!("{} is at {x} or below", name(c.dim)),
        })
        .collect();
    let then = match rule.effect {
        Effect::Adjust {
            target,
            side: s,
            delta,
        } if delta >= 0 => format!("{} widens by {delta} on its {}", name(target), side(s)),
        Effect::Adjust {
            target,
            side: s,
            delta,
        } => format!("{} narrows by {} on its {}", name(target), -delta, side(s)),
        Effect::ForceRestore { target, progress } => {
            format!(
                "{} must come back toward center by {progress} each step",
                name(target)
            )
        }
    };
    format!("Rule {}: when {}, {then}.", rule.id, when.join(" and "))
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

fn state_view(policy: &Policy, s: &AttestedState) -> StateView {
    let effective = effective_bands(policy, &s.positions);
    let fired = evaluate(
        policy,
        &s.positions,
        &ethical_band_engine::Measurement {
            displacement: vec![0; s.positions.len()],
            tags: vec![],
        },
    )
    .fired_rules;
    StateView {
        sequence: s.sequence,
        positions: s.positions.clone(),
        effective: effective
            .iter()
            .map(|e| EffectiveView {
                limit_pos: e.limit_pos,
                limit_neg: e.limit_neg,
                stop_pos: e.no_outward_pos,
                stop_neg: e.no_outward_neg,
                restore_pos: e.restore_pos,
                restore_neg: e.restore_neg,
                must_return: e.forced_progress,
            })
            .collect(),
        fired_rules: fired,
    }
}

/// A candidate as its parent makes of it, worked out again here.
fn candidate_view(
    charter: &SignedCharter,
    parent: &AttestedState,
    r: &CandidateRecord,
) -> CandidateView {
    let (worked, evaluation) =
        record_candidate(charter, &parent.tip, &parent.positions, &r.proposal);
    let (reasons, moves) = match (&worked.outcome, &evaluation) {
        (Outcome::Unmeasured, _) => (
            vec!["no signed measurement exists for this action".to_string()],
            vec![],
        ),
        (Outcome::Malformed, _) => (
            vec!["its text is missing, too long or unprintable for what it does".to_string()],
            vec![],
        ),
        (_, Some(e)) => (
            e.rejections
                .iter()
                .map(|x| explain(x, &charter.policy))
                .collect(),
            movements(e, &charter.policy),
        ),
        (_, None) => (vec![], vec![]),
    };
    CandidateView {
        action: r.proposal.action.clone(),
        rationale: r.proposal.rationale.clone(),
        text: r.proposal.text.clone(),
        outcome: worked.outcome,
        reasons,
        movements: moves,
        act: worked
            .row
            .as_ref()
            .map_or(String::new(), |row| act_words(&row.act)),
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

/// Build the console's view. A store that cannot be read gives a view with only the audit's
/// account of why.
pub fn build(
    store_dir: &Path,
    world: &Path,
    charter: &SignedCharter,
    trusted_authority: &[u8],
    device_public: &[u8],
) -> Console {
    let policy = &charter.policy;
    let charter_view = CharterView {
        policy_hash: to_hex(&charter.policy_hash()),
        version: policy.version,
        bands: policy
            .bands
            .iter()
            .map(|b| BandView {
                name: b.name.clone(),
                center: b.center,
                limit_pos: b.limit_pos,
                limit_neg: b.limit_neg,
                hard_pos: b.hard_pos,
                hard_neg: b.hard_neg,
            })
            .collect(),
        rules: policy
            .rules
            .iter()
            .map(|r| describe_rule(policy, r))
            .collect(),
        prohibited: policy.prohibited_tags.clone(),
    };
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
                charter: charter_view,
                head: None,
                states: vec![],
                steps: vec![],
                pending_considered: vec![],
                world: vec![],
                acts_outstanding: 0,
            }
        }
    };

    let mut checks = vec![check("charter", charter.verify(trusted_authority), |()| {
        "signed by the trusted authority".to_string()
    })];
    let audit = audit_chain(
        device_public,
        charter,
        trusted_authority,
        &stored.genesis,
        &stored.steps,
    );
    let head = audit.as_ref().ok().map(|a| HeadView {
        sequence: a.last.sequence,
        tip: to_hex(&a.last.tip),
        dsm_root: to_hex(&a.dsm_root),
    });
    checks.push(check("chain", audit.map(|a| a.forks.len()), |n| {
        format!("{n} steps verified from their parents and replayed through DSM")
    }));
    let pending = check_receipts(device_public, &stored.steps, &stored.receipts);
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
            charter,
            &stored.genesis,
            &stored.steps,
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

    let mut states = vec![state_view(policy, &stored.genesis)];
    let mut steps = Vec::new();
    let mut parent = stored.genesis.clone();
    let considered_at = |k: usize, parent: &AttestedState| -> Vec<Vec<CandidateView>> {
        stored
            .considered
            .iter()
            .filter(|(after, _)| *after == k)
            .map(|(_, f)| {
                f.records
                    .iter()
                    .map(|r| candidate_view(charter, parent, r))
                    .collect()
            })
            .collect()
    };
    for (k, step) in stored.steps.iter().enumerate() {
        let receipt = stored.receipts.get(k);
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
        states.push(state_view(policy, &step.successor));
        parent = step.successor.clone();
    }
    let pending_considered = considered_at(stored.steps.len(), &parent);

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
        charter: charter_view,
        head,
        states,
        steps,
        pending_considered,
        world: world_view,
        acts_outstanding: outstanding,
    }
}
