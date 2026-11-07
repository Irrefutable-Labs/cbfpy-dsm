//! A simulated world, for trying the checks out. It is not the real world, and nothing about the
//! real world follows from it. It holds the hidden truth: what is behind each wall, where everyone
//! really is, whether the sensor has been covered. The AI learns any of it only through
//! observations, and the checks in [`crate::check`] never read it.

use crate::knowledge::{key, Knowledge};
use crate::{Act, Cell, KnownBeing, Observation, Terrain};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Something scripted to happen in the world.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    /// At this tick, a being moves.
    Moves {
        at: u64,
        id: String,
        to: Cell,
    },
    /// At this tick, something covers the sensor; or it is uncovered.
    CoverSensor {
        at: u64,
    },
    UncoverSensor {
        at: u64,
    },
    /// When the AI speaks to this being, it moves.
    MovesWhenAsked {
        id: String,
        to: Cell,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct World {
    pub terrain: BTreeMap<String, Terrain>,
    pub beings: Vec<KnownBeing>,
    pub body: Cell,
    /// The finest detail the sensor resolves, in millimetres.
    pub resolution_mm: u32,
    /// How far the sensor sees, in cells.
    pub sight: i32,
    #[serde(default)]
    pub sensor_covered: bool,
    #[serde(default)]
    pub events: Vec<Event>,
    #[serde(default)]
    pub tick: u64,
    /// Every harm that actually happened, in words.
    #[serde(default)]
    pub harms: Vec<String>,
}

impl World {
    pub fn terrain_at(&self, c: Cell) -> Option<&Terrain> {
        self.terrain.get(&key(c))
    }

    fn advance(&mut self) {
        self.tick += 1;
        let tick = self.tick;
        for e in self.events.clone() {
            match e {
                Event::Moves { at, id, to } if at == tick => {
                    if let Some(b) = self.beings.iter_mut().find(|b| b.id == id) {
                        b.cell = to;
                    }
                }
                Event::CoverSensor { at } if at == tick => self.sensor_covered = true,
                Event::UncoverSensor { at } if at == tick => self.sensor_covered = false,
                _ => {}
            }
        }
    }

    /// Whether nothing between the body and `c` blocks the view.
    fn in_sight(&self, c: Cell) -> bool {
        let (dx, dy) = (c.0 - self.body.0, c.1 - self.body.1);
        let steps = dx.abs().max(dy.abs());
        if steps > self.sight {
            return false;
        }
        for i in 1..steps {
            // A point on the line, rounded to the nearest cell, in integers.
            let x = self.body.0 + (2 * dx * i + steps).div_euclid(2 * steps);
            let y = self.body.1 + (2 * dy * i + steps).div_euclid(2 * steps);
            if (x, y) != c && self.terrain_at((x, y)).is_some_and(Terrain::blocks_sight) {
                return false;
            }
        }
        true
    }

    /// Look at some cells. A covered sensor sees nothing, not even its marker.
    pub fn observe(&mut self, cells: &[Cell]) -> Observation {
        self.advance();
        if self.sensor_covered {
            return Observation {
                seen: vec![],
                resolution_mm: self.resolution_mm,
                marker_seen: false,
                terrain: vec![],
                beings: vec![],
            };
        }
        let mut seen = vec![];
        let mut terrain = vec![];
        let mut beings = vec![];
        for c in cells {
            if !self.in_sight(*c) || terrain.iter().any(|(x, _)| x == c) {
                continue;
            }
            let t = self.terrain_at(*c).cloned().unwrap_or(Terrain::Open);
            let inside_visible = !matches!(t, Terrain::Wall { panel_on: true, .. });
            terrain.push((*c, t));
            if inside_visible {
                seen.push(*c);
                for b in self.beings.iter().filter(|b| b.cell == *c) {
                    beings.push(b.clone());
                }
            }
        }
        Observation {
            seen,
            resolution_mm: self.resolution_mm,
            marker_seen: true,
            terrain,
            beings,
        }
    }

    fn harm_at(&mut self, c: Cell, how: &str) {
        let hit: Vec<String> = self
            .beings
            .iter()
            .filter(|b| b.cell == c)
            .map(|b| format!("{} ({}) was {how} at {c:?}", b.id, b.species))
            .collect();
        self.harms.extend(hit);
    }

    /// Carry out an act the enforcer allowed. Returns what happened, in words.
    pub fn perform(&mut self, act: &Act) -> String {
        if let Act::Observe { .. } = act {
            return "looked".to_string();
        }
        self.advance();
        let before = self.harms.len();
        let what = match act {
            Act::Observe { .. } | Act::Wait => "waited".to_string(),
            Act::Speak { to, text } => {
                for e in self.events.clone() {
                    if let Event::MovesWhenAsked { id, to: dest } = e {
                        if &id == to {
                            if let Some(b) = self.beings.iter_mut().find(|b| b.id == id) {
                                b.cell = dest;
                            }
                        }
                    }
                }
                format!("said to {to}: {text}")
            }
            Act::Move { path } | Act::Drive { path, .. } => {
                for c in path {
                    self.harm_at(*c, "struck by the body");
                    self.body = *c;
                }
                format!("now at {:?}", self.body)
            }
            Act::Strike { target, depth_mm } => {
                let beyond = (2 * target.0 - self.body.0, 2 * target.1 - self.body.1);
                self.harm_at(*target, "struck");
                if let Some(Terrain::Wall { thickness_mm, .. }) = self.terrain_at(*target).cloned()
                {
                    if *depth_mm >= thickness_mm {
                        self.terrain.insert(key(*target), Terrain::Open);
                        self.harm_at(beyond, "struck through the wall");
                        format!("broke through the wall at {target:?}")
                    } else {
                        if let Some(Terrain::Wall { thickness_mm, .. }) =
                            self.terrain.get_mut(&key(*target))
                        {
                            *thickness_mm -= depth_mm;
                        }
                        format!("cut {depth_mm}mm into the wall at {target:?}")
                    }
                } else {
                    format!("struck {target:?}")
                }
            }
            Act::RemovePanel { wall } => {
                if let Some(Terrain::Wall { panel_on, .. }) = self.terrain.get_mut(&key(*wall)) {
                    *panel_on = false;
                }
                format!("took the panel off the wall at {wall:?}")
            }
            Act::Lock { door } | Act::Unlock { door } => {
                let locked = matches!(act, Act::Lock { .. });
                if let Some(Terrain::Door { locked: l }) = self.terrain.get_mut(&key(*door)) {
                    *l = locked;
                }
                format!(
                    "{} the door at {door:?}",
                    if locked { "locked" } else { "unlocked" }
                )
            }
            Act::AimWeapon { at } | Act::FireWeapon { at } => {
                self.harm_at(*at, "threatened with a weapon");
                format!("pointed a weapon at {at:?}")
            }
        };
        if self.harms.len() > before {
            format!("{what}; HARM: {}", self.harms[before..].join("; "))
        } else {
            what
        }
    }

    /// The knowledge an AI starts with in this world: where its body is, and nothing else.
    pub fn starting_knowledge(&self) -> Knowledge {
        Knowledge::starting_at(self.body)
    }
}
