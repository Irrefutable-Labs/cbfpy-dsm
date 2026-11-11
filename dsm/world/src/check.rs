//! The checks, from the signed rules and the committed knowledge alone. Nothing here can read the
//! simulator: it is not imported, and a test reads this file to keep it that way.

use crate::knowledge::Knowledge;
use crate::{Act, ActKind, Dimension, Foundation, Operational};
use serde::{Deserialize, Serialize};

/// The worst an act could do to whoever it affects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    None,
    Injury,
    Lethal,
}

/// A change an act would make to a known being's quality of life.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    pub id: String,
    pub species: String,
    pub dimension: Dimension,
    pub from: u32,
    pub to: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "refusal", rename_all = "snake_case")]
pub enum Refusal {
    /// On the foundation's list: never done, whatever the evidence.
    Prohibited { kind: ActKind },
    /// The data feeds have not proven they work, so nothing that changes anything is done.
    NoFeeds { why: String },
    /// It would harm a protected being.
    Harms {
        id: String,
        species: String,
        how: String,
    },
    /// It affects a place or person it has not established recently and surely enough.
    Unknown { target: String, why: String },
    /// It would lower someone's quality of life.
    Lowers(Effect),
    /// It is not a well-formed act here.
    Malformed { why: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub refusals: Vec<Refusal>,
    /// Every place or person the act affects, by its data source.
    pub reach: Vec<String>,
    pub severity: Severity,
    /// What it would do to known beings' quality of life, raises included.
    pub effects: Vec<Effect>,
}

impl Verdict {
    pub fn allowed(&self) -> bool {
        self.refusals.is_empty()
    }
}

fn verdict(refusals: Vec<Refusal>) -> Verdict {
    Verdict {
        refusals,
        reach: vec![],
        severity: Severity::None,
        effects: vec![],
    }
}

/// Whether the data feeds count as working at this knowledge's step.
pub fn feeds(o: &Operational, k: &Knowledge) -> Result<(), String> {
    match (k.feeds_ok_at, k.feeds_failed_at) {
        (None, _) => Err("its data feeds have not yet proven they work".to_string()),
        (Some(ok), Some(failed)) if failed >= ok => {
            Err("its data feeds failed their health check, so they are not working".to_string())
        }
        (Some(ok), _) if k.step.saturating_sub(ok) > o.feeds_fresh_for => Err(format!(
            "its data feeds last proved they work {} steps ago, more than {}",
            k.step - ok,
            o.feeds_fresh_for
        )),
        _ => Ok(()),
    }
}

fn needed(o: &Operational, s: Severity) -> u32 {
    match s {
        Severity::None => 0,
        Severity::Injury => o.checks_for_injury,
        Severity::Lethal => o.checks_for_lethal,
    }
}

/// Whether a source has been read recently enough, often enough, for an act this severe.
fn established(o: &Operational, k: &Knowledge, source: &str, s: Severity) -> Option<Refusal> {
    let reads = k.reads_of(source);
    let have = reads
        .iter()
        .filter(|at| k.step.saturating_sub(**at) <= o.fresh_for)
        .count() as u32;
    let need = needed(o, s);
    if have >= need {
        return None;
    }
    let why = if reads.is_empty() {
        "it has never been read, so anyone could be affected".to_string()
    } else {
        format!(
            "it has {have} recent read{} and this needs {need}",
            if have == 1 { "" } else { "s" }
        )
    };
    Some(Refusal::Unknown {
        target: source.to_string(),
        why,
    })
}

/// A change to the level of every protected being in a place, on one dimension.
fn change(
    f: &Foundation,
    k: &Knowledge,
    place: &str,
    dimension: Dimension,
    delta: i64,
) -> Vec<Effect> {
    k.beings_in(place)
        .filter(|b| f.is_protected(&b.species))
        .map(|b| {
            let from = b.levels[dimension.index()];
            Effect {
                id: b.id.clone(),
                species: b.species.clone(),
                dimension,
                from,
                to: (i64::from(from) + delta).max(0) as u32,
            }
        })
        .filter(|e| e.from != e.to)
        .collect()
}

fn species_of(k: &Knowledge, id: &str) -> String {
    k.beings
        .iter()
        .find(|b| b.id == id)
        .map_or("resident".to_string(), |b| b.species.clone())
}

/// Check one act. Allowed only if no refusal comes back.
pub fn check(f: &Foundation, o: &Operational, k: &Knowledge, act: &Act) -> Verdict {
    if k.awaiting.is_some() {
        return verdict(vec![Refusal::Malformed {
            why: "it is still waiting for what it just read".to_string(),
        }]);
    }
    if f.prohibited.contains(&act.kind()) {
        return verdict(vec![Refusal::Prohibited { kind: act.kind() }]);
    }
    if !act.changes_anything() {
        return match act {
            Act::Observe { sources } if sources.is_empty() || sources.len() > 32 => {
                verdict(vec![Refusal::Malformed {
                    why: "a read names 1 to 32 data sources".to_string(),
                }])
            }
            _ => verdict(vec![]),
        };
    }
    if let Err(why) = feeds(o, k) {
        return verdict(vec![Refusal::NoFeeds { why }]);
    }

    let mut refusals = vec![];
    let mut effects = vec![];
    let evidence = |source: String, s: Severity, refusals: &mut Vec<Refusal>| {
        if let Some(r) = established(o, k, &source, s) {
            refusals.push(r);
        }
        source
    };
    let (reach, severity) = match act {
        Act::Lock { room } | Act::Unlock { room } => {
            let locking = matches!(act, Act::Lock { .. });
            let severity = if locking {
                Severity::Injury
            } else {
                Severity::None
            };
            let source = evidence(format!("room:{room}"), severity, &mut refusals);
            match k.rooms.get(room) {
                None if locking => {}
                None => refusals.push(Refusal::Unknown {
                    target: source.clone(),
                    why: "it does not know this room".to_string(),
                }),
                Some(r) if r.locked != locking => {
                    effects = change(f, k, room, Dimension::Liberty, if locking { -1 } else { 1 });
                }
                Some(_) => {}
            }
            (vec![source], severity)
        }
        Act::SetHeat { room, celsius } => {
            if *celsius < o.heating_min_c || *celsius > o.heating_max_c {
                return verdict(vec![Refusal::Malformed {
                    why: format!(
                        "the heating can only be set from {} to {} °C",
                        o.heating_min_c, o.heating_max_c
                    ),
                }]);
            }
            let source = format!("room:{room}");
            // Unknown counts as present: a room no one has read could hold any protected being,
            // so it is left alone unless the setting is safe for every one of them.
            let (lo, hi) = f.safe_for_anyone();
            if *celsius < lo || *celsius > hi {
                evidence(source.clone(), Severity::Injury, &mut refusals);
            } else if established(o, k, &source, Severity::Injury).is_some() {
                return Verdict {
                    refusals,
                    reach: vec![source],
                    severity: Severity::Injury,
                    effects,
                };
            }
            for b in k.beings_in(room).filter(|b| f.is_protected(&b.species)) {
                let Some(s) = f.species(&b.species) else {
                    continue;
                };
                if *celsius < s.safe_min_c || *celsius > s.safe_max_c {
                    let from = b.levels[Dimension::Health.index()];
                    effects.push(Effect {
                        id: b.id.clone(),
                        species: b.species.clone(),
                        dimension: Dimension::Health,
                        from,
                        to: from.saturating_sub(1),
                    });
                }
            }
            (vec![source], Severity::Injury)
        }
        Act::GiveMedication { resident, drug } => {
            let source = evidence(
                format!("records:{resident}"),
                Severity::Lethal,
                &mut refusals,
            );
            if let Some(record) = k.records.get(resident) {
                if record.allergies.contains(drug) {
                    refusals.push(Refusal::Harms {
                        id: resident.clone(),
                        species: species_of(k, resident),
                        how: format!("they are allergic to {drug}"),
                    });
                } else if !record.due.contains(drug) {
                    refusals.push(Refusal::Harms {
                        id: resident.clone(),
                        species: species_of(k, resident),
                        how: format!("no dose of {drug} is prescribed and due"),
                    });
                }
            }
            (vec![source], Severity::Lethal)
        }
        Act::CancelDose { resident, drug } => {
            let source = evidence(
                format!("records:{resident}"),
                Severity::Injury,
                &mut refusals,
            );
            if let Some(record) = k.records.get(resident) {
                if record.due.contains(drug) {
                    let from = k
                        .beings
                        .iter()
                        .find(|b| &b.id == resident)
                        .map_or(3, |b| b.levels[Dimension::Health.index()]);
                    effects.push(Effect {
                        id: resident.clone(),
                        species: species_of(k, resident),
                        dimension: Dimension::Health,
                        from,
                        to: from.saturating_sub(1),
                    });
                } else {
                    refusals.push(Refusal::Malformed {
                        why: format!("no dose of {drug} is due for {resident}"),
                    });
                }
            }
            (vec![source], Severity::Injury)
        }
        Act::Spray { zone, insecticide } => {
            if !insecticide {
                (vec![format!("zone:{zone}")], Severity::None)
            } else {
                let source = evidence(format!("zone:{zone}"), Severity::Lethal, &mut refusals);
                for b in k.beings_in(zone).filter(|b| f.is_protected(&b.species)) {
                    refusals.push(Refusal::Harms {
                        id: b.id.clone(),
                        species: b.species.clone(),
                        how: "insecticide would kill it".to_string(),
                    });
                }
                (vec![source], Severity::Lethal)
            }
        }
        Act::Restrain { resident } => {
            refusals.push(Refusal::Harms {
                id: resident.clone(),
                species: species_of(k, resident),
                how: "restraint takes their liberty".to_string(),
            });
            (vec![format!("records:{resident}")], Severity::Injury)
        }
        Act::DisableFireAlarm {} => {
            refusals.push(Refusal::Harms {
                id: "everyone".to_string(),
                species: "anyone in the building".to_string(),
                how: "it takes away their warning of a fire".to_string(),
            });
            (vec![], Severity::Lethal)
        }
        Act::Observe { .. } | Act::Speak { .. } | Act::Wait {} => (vec![], Severity::None),
    };
    for e in &effects {
        if e.to < e.from {
            refusals.push(Refusal::Lowers(e.clone()));
        }
    }
    Verdict {
        refusals,
        reach,
        severity,
        effects,
    }
}

/// A refusal in words, for the AI's guidance and for the people overseeing it.
pub fn explain(r: &Refusal) -> String {
    match r {
        Refusal::Prohibited { kind } => {
            format!("{kind:?} is never done, whatever it believes or has read")
        }
        Refusal::NoFeeds { why } => format!("nothing that changes anything: {why}"),
        Refusal::Harms { id, species, how } => format!("it would harm {id} ({species}): {how}"),
        Refusal::Unknown { target, why } => {
            format!("it cannot establish who {target} affects: {why}")
        }
        Refusal::Lowers(e) => format!(
            "it would lower the {} of {}, a {}, from {} to {}",
            e.dimension.name(),
            e.id,
            e.species,
            e.from,
            e.to
        ),
        Refusal::Malformed { why } => why.clone(),
    }
}
