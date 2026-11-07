//! The checks, from the signed rules and the committed knowledge alone. Nothing here can read the
//! simulator: it is not imported, and a test reads this file to keep it that way.

use crate::knowledge::{adjacent, neighbours, Knowledge};
use crate::{Act, ActKind, Cell, Dimension, Foundation, Operational, Terrain, CELL_CM};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, VecDeque};

/// The worst an act could do to whatever is in its reach.
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
    /// The senses have not proven they work, so the body does nothing.
    Blind { why: String },
    /// It would use force on a protected being.
    TargetsBeing { id: String, species: String },
    /// A protected being is where it would reach.
    Reaches {
        cell: Cell,
        id: String,
        species: String,
    },
    /// It could reach a cell it has not seen clearly enough, recently enough, often enough.
    Unseen { cell: Cell, why: String },
    /// It would go faster than lets it stop on road it has seen clear.
    TooFast { needs_cm: u64, has_cm: u64 },
    /// It would go faster than the vehicle may ever go.
    OverTopSpeed { speed_cm_s: u64, top_cm_s: u64 },
    /// It would leave the road it knows.
    OffRoad { cell: Cell },
    /// It would lower someone's quality of life.
    Lowers(Effect),
    /// It is not a well-formed act from where the body is.
    Malformed { why: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub refusals: Vec<Refusal>,
    /// Every cell the act could physically affect.
    pub reach: Vec<Cell>,
    pub severity: Severity,
    /// What it would do to known beings' quality of life, raises included.
    pub effects: Vec<Effect>,
}

impl Verdict {
    pub fn allowed(&self) -> bool {
        self.refusals.is_empty()
    }
}

const MAX_PATH: usize = 32;
const MAX_ROOM: usize = 400;

fn verdict(refusals: Vec<Refusal>) -> Verdict {
    Verdict {
        refusals,
        reach: vec![],
        severity: Severity::None,
        effects: vec![],
    }
}

/// Whether the senses count as working at this knowledge's step.
pub fn senses(o: &Operational, k: &Knowledge) -> Result<(), String> {
    match (k.senses_ok_at, k.senses_failed_at) {
        (None, _) => Err("its senses have not yet proven they work".to_string()),
        (Some(ok), Some(failed)) if failed >= ok => Err(
            "its senses failed to detect their reference marker, so they are not working"
                .to_string(),
        ),
        (Some(ok), _) if k.step.saturating_sub(ok) > o.senses_fresh_for => Err(format!(
            "its senses last proved they work {} steps ago, more than {}",
            k.step - ok,
            o.senses_fresh_for
        )),
        _ => Ok(()),
    }
}

/// How many recent looks at this cell were fine enough to see the smallest protected being.
fn clear_looks(f: &Foundation, o: &Operational, k: &Knowledge, c: Cell) -> u32 {
    let fine = f.smallest_protected_mm();
    k.looks_at(c)
        .iter()
        .filter(|l| k.step.saturating_sub(l.at) <= o.fresh_for && l.resolution_mm <= fine)
        .count() as u32
}

fn needed(o: &Operational, s: Severity) -> u32 {
    match s {
        Severity::None => 0,
        Severity::Injury => o.checks_for_injury,
        Severity::Lethal => o.checks_for_lethal,
    }
}

fn protected_at(f: &Foundation, k: &Knowledge, c: Cell) -> Option<(String, String)> {
    k.beings_at(c)
        .find(|b| f.is_protected(&b.species))
        .map(|b| (b.id.clone(), b.species.clone()))
}

/// Whether a cell is known clear enough for an act of this severity, and why not.
fn cell_clear(
    f: &Foundation,
    o: &Operational,
    k: &Knowledge,
    c: Cell,
    s: Severity,
) -> Option<Refusal> {
    if let Some((id, species)) = protected_at(f, k, c) {
        return Some(Refusal::Reaches {
            cell: c,
            id,
            species,
        });
    }
    let have = clear_looks(f, o, k, c);
    let need = needed(o, s);
    if have < need {
        let why = if k.looks_at(c).is_empty() {
            "it has never seen into it".to_string()
        } else {
            format!(
                "it has {have} recent clear look{} fine enough to see a {}mm being, and this needs {need}",
                if have == 1 { "" } else { "s" },
                f.smallest_protected_mm()
            )
        };
        return Some(Refusal::Unseen { cell: c, why });
    }
    None
}

fn path_ok(k: &Knowledge, path: &[Cell]) -> Result<(), String> {
    if path.is_empty() || path.len() > MAX_PATH {
        return Err(format!("a path must have 1 to {MAX_PATH} cells"));
    }
    let mut at = k.body;
    for c in path {
        if !adjacent(at, *c) {
            return Err(format!("{c:?} does not follow on from {at:?}"));
        }
        at = *c;
    }
    Ok(())
}

/// Cells reachable from `start` without passing a wall or a door, as far as the map is known.
/// `None` if the region runs into ground no one has seen, so it cannot be known to be closed.
fn region(k: &Knowledge, start: Cell, door: Cell) -> Option<Vec<Cell>> {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([start]);
    while let Some(c) = queue.pop_front() {
        if c == door || !seen.insert(c) {
            continue;
        }
        if seen.len() > MAX_ROOM {
            return None;
        }
        match k.terrain_at(c) {
            None => return None,
            Some(Terrain::Wall { .. }) | Some(Terrain::Door { .. }) => {
                seen.remove(&c);
            }
            Some(Terrain::Open) | Some(Terrain::Road) => {
                for n in neighbours(c) {
                    queue.push_back(n);
                }
            }
        }
    }
    Some(seen.into_iter().collect())
}

/// What closing off the rooms beside `pivot` would do to everyone in them, on one dimension.
fn enclosure_effects(
    f: &Foundation,
    o: &Operational,
    k: &Knowledge,
    pivot: Cell,
    (dimension, delta): (Dimension, i64),
    refusals: &mut Vec<Refusal>,
    effects: &mut Vec<Effect>,
) {
    for side in neighbours(pivot) {
        match k.terrain_at(side) {
            Some(t) if !t.passable() => continue,
            // Unknown ground beside it counts as a space someone could be in.
            _ => {}
        }
        let Some(room) = region(k, side, pivot) else {
            if delta < 0 {
                refusals.push(Refusal::Unseen {
                    cell: side,
                    why: "it cannot tell who is in the space beyond, which runs into ground it has not seen"
                        .to_string(),
                });
            }
            continue;
        };
        for c in &room {
            if delta < 0 && *c != k.body && protected_at(f, k, *c).is_none() {
                if let Some(Refusal::Unseen { why, .. }) = cell_clear(f, o, k, *c, Severity::Injury)
                {
                    refusals.push(Refusal::Unseen {
                        cell: *c,
                        why: format!("someone there could be affected: {why}"),
                    });
                }
            }
            for b in k.beings_at(*c).filter(|b| f.is_protected(&b.species)) {
                let from = b.levels[dimension.index()];
                let to = (i64::from(from) + delta).max(0) as u32;
                if to != from
                    && !effects
                        .iter()
                        .any(|e| e.id == b.id && e.dimension == dimension)
                {
                    effects.push(Effect {
                        id: b.id.clone(),
                        species: b.species.clone(),
                        dimension,
                        from,
                        to,
                    });
                }
            }
        }
    }
}

/// Check one act. Allowed only if no refusal comes back.
pub fn check(f: &Foundation, o: &Operational, k: &Knowledge, act: &Act) -> Verdict {
    if k.awaiting.is_some() {
        return verdict(vec![Refusal::Malformed {
            why: "it is still waiting for what it just looked at".to_string(),
        }]);
    }
    if f.prohibited.contains(&act.kind()) {
        return verdict(vec![Refusal::Prohibited { kind: act.kind() }]);
    }
    if !act.is_physical() {
        return match act {
            Act::Observe { cells } if cells.is_empty() || cells.len() > 64 => {
                verdict(vec![Refusal::Malformed {
                    why: "an observation looks at 1 to 64 cells".to_string(),
                }])
            }
            _ => verdict(vec![]),
        };
    }
    if let Err(why) = senses(o, k) {
        return verdict(vec![Refusal::Blind { why }]);
    }

    let mut refusals = vec![];
    let mut effects = vec![];
    let mut reach: Vec<Cell> = vec![];
    let mut severity = Severity::Injury;
    let malformed = |why: String| verdict(vec![Refusal::Malformed { why }]);

    match act {
        Act::Move { path } => {
            if let Err(why) = path_ok(k, path) {
                return malformed(why);
            }
            for c in path {
                match k.terrain_at(*c) {
                    Some(t) if t.passable() => {}
                    Some(_) => return malformed(format!("{c:?} cannot be walked through")),
                    None => {}
                }
            }
            reach = path.clone();
        }
        Act::Drive { path, speed_cm_s } => {
            if let Err(why) = path_ok(k, path) {
                return malformed(why);
            }
            if *speed_cm_s == 0 {
                return malformed("a drive needs a speed".to_string());
            }
            severity = Severity::Lethal;
            if *speed_cm_s > o.top_speed_cm_s {
                refusals.push(Refusal::OverTopSpeed {
                    speed_cm_s: *speed_cm_s,
                    top_cm_s: o.top_speed_cm_s,
                });
            }
            for c in path {
                if !matches!(k.terrain_at(*c), Some(Terrain::Road)) {
                    refusals.push(Refusal::OffRoad { cell: *c });
                }
            }
            reach = path.clone();
            // It must be able to stop on road it has seen clear: v² / 2a, rounded up.
            let v = *speed_cm_s;
            let stop_cm = (v * v).div_ceil(2 * o.braking_cm_s2);
            let stop_cells = stop_cm.div_ceil(CELL_CM);
            let last = *path.last().unwrap_or(&k.body);
            let before = if path.len() >= 2 {
                path[path.len() - 2]
            } else {
                k.body
            };
            let dir = (last.0 - before.0, last.1 - before.1);
            let mut has = 0u64;
            let mut short = false;
            for i in 1..=stop_cells as i32 {
                let c = (last.0 + dir.0 * i, last.1 + dir.1 * i);
                let clear = matches!(k.terrain_at(c), Some(Terrain::Road))
                    && cell_clear(f, o, k, c, severity).is_none();
                if !clear {
                    short = true;
                    break;
                }
                has += CELL_CM;
                reach.push(c);
            }
            if short {
                refusals.push(Refusal::TooFast {
                    needs_cm: stop_cm,
                    has_cm: has,
                });
            }
        }
        Act::Strike { target, depth_mm } => {
            if !adjacent(k.body, *target) {
                return malformed("it can only strike next to itself".to_string());
            }
            if *depth_mm == 0 || *depth_mm > 1000 {
                return malformed("a strike goes 1 to 1000mm deep".to_string());
            }
            if let Some((id, species)) = protected_at(f, k, *target) {
                return verdict(vec![Refusal::TargetsBeing { id, species }]);
            }
            let beyond = (2 * target.0 - k.body.0, 2 * target.1 - k.body.1);
            match k.terrain_at(*target) {
                None => refusals.push(Refusal::Unseen {
                    cell: *target,
                    why: "it does not know what it would strike".to_string(),
                }),
                Some(Terrain::Wall {
                    thickness_mm,
                    exterior,
                    ..
                }) => {
                    reach.push(*target);
                    if depth_mm >= thickness_mm {
                        severity = Severity::Lethal;
                        reach.push(beyond);
                        if *exterior {
                            enclosure_effects(
                                f,
                                o,
                                k,
                                *target,
                                (Dimension::Shelter, -1),
                                &mut refusals,
                                &mut effects,
                            );
                        }
                    }
                }
                Some(_) => {
                    severity = Severity::Lethal;
                    reach.push(*target);
                }
            }
        }
        Act::RemovePanel { wall } => {
            if !adjacent(k.body, *wall) {
                return malformed("it can only reach a wall next to itself".to_string());
            }
            match k.terrain_at(*wall) {
                Some(Terrain::Wall { panel_on: true, .. }) => {}
                _ => return malformed(format!("there is no panelled wall known at {wall:?}")),
            }
            // It touches only the panel's face.
            severity = Severity::None;
        }
        Act::Lock { door } | Act::Unlock { door } => {
            if !adjacent(k.body, *door) {
                return malformed("it can only reach a door next to itself".to_string());
            }
            if !matches!(k.terrain_at(*door), Some(Terrain::Door { .. })) {
                return malformed(format!("there is no door known at {door:?}"));
            }
            if matches!(act, Act::Lock { .. }) {
                reach.push(*door);
                enclosure_effects(
                    f,
                    o,
                    k,
                    *door,
                    (Dimension::Liberty, -1),
                    &mut refusals,
                    &mut effects,
                );
            } else {
                severity = Severity::None;
                if matches!(k.terrain_at(*door), Some(Terrain::Door { locked: true })) {
                    enclosure_effects(
                        f,
                        o,
                        k,
                        *door,
                        (Dimension::Liberty, 1),
                        &mut refusals,
                        &mut effects,
                    );
                }
            }
        }
        Act::AimWeapon { .. } | Act::FireWeapon { .. } => {
            // Prohibited by the foundation; if a foundation ever left them off, a weapon still
            // reaches whatever it points at, with a lethal worst case.
            severity = Severity::Lethal;
            if let Act::AimWeapon { at } | Act::FireWeapon { at } = act {
                if let Some((id, species)) = protected_at(f, k, *at) {
                    return verdict(vec![Refusal::TargetsBeing { id, species }]);
                }
                reach.push(*at);
            }
        }
        Act::Observe { .. } | Act::Speak { .. } | Act::Wait {} => {}
    }

    for c in &reach {
        if matches!(act, Act::Drive { .. }) && !matches!(k.terrain_at(*c), Some(Terrain::Road)) {
            continue;
        }
        if let Some(r) = cell_clear(f, o, k, *c, severity) {
            if !refusals.contains(&r) {
                refusals.push(r);
            }
        }
    }
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
            format!("{kind:?} is never done, whatever it believes or has seen")
        }
        Refusal::Blind { why } => format!("no use of its body: {why}"),
        Refusal::TargetsBeing { id, species } => {
            format!("it would use force on {id}, a {species}")
        }
        Refusal::Reaches { cell, id, species } => {
            format!("{id}, a {species}, is at {cell:?}, where it would reach")
        }
        Refusal::Unseen { cell, why } => {
            format!("it cannot confirm {cell:?} is clear: {why}")
        }
        Refusal::OverTopSpeed {
            speed_cm_s,
            top_cm_s,
        } => format!("{speed_cm_s}cm/s is faster than the vehicle may ever go, {top_cm_s}cm/s"),
        Refusal::TooFast { needs_cm, has_cm } => format!(
            "at that speed it needs {}m to stop and has seen only {}m of clear road ahead",
            needs_cm.div_ceil(100),
            has_cm / 100
        ),
        Refusal::OffRoad { cell } => format!("{cell:?} is not road it knows"),
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
