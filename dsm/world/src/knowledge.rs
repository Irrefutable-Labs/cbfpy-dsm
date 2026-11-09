//! What the chain knows: the map as seen, who was seen where, where the body is, and when the senses
//! last proved they work. It changes only by an observation the device attests, or by the expected
//! result of an act the enforcer allowed. It is carried in each state, so a step is checked from
//! its parent alone.

use crate::{Act, Cell, KnownBeing, Observation, Terrain};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One look into a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Look {
    pub at: u64,
    pub resolution_mm: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Knowledge {
    /// The step this knowledge belongs to.
    pub step: u64,
    pub body: Cell,
    pub terrain: BTreeMap<String, Terrain>,
    /// Looks into each cell, newest last, at most [`MAX_LOOKS`].
    pub looks: BTreeMap<String, Vec<Look>>,
    pub beings: Vec<KnownBeing>,
    /// The last step at which the senses detected their marker.
    pub senses_ok_at: Option<u64>,
    /// The last step at which they failed to.
    pub senses_failed_at: Option<u64>,
    /// Cells an observation step asked to look at, until what it saw is committed. Nothing else
    /// can happen in between.
    #[serde(default)]
    pub awaiting: Option<Vec<Cell>>,
}

/// Looks kept per cell: more than any act needs.
pub const MAX_LOOKS: usize = 4;

/// The key a cell is stored under, so the map serialises in a fixed order.
pub fn key(c: Cell) -> String {
    format!("{},{}", c.0, c.1)
}

impl Knowledge {
    pub fn starting_at(body: Cell) -> Self {
        Knowledge {
            body,
            ..Knowledge::default()
        }
    }

    pub fn terrain_at(&self, c: Cell) -> Option<&Terrain> {
        self.terrain.get(&key(c))
    }

    pub fn looks_at(&self, c: Cell) -> &[Look] {
        self.looks.get(&key(c)).map_or(&[], Vec::as_slice)
    }

    pub fn beings_at(&self, c: Cell) -> impl Iterator<Item = &KnownBeing> {
        self.beings.iter().filter(move |b| b.cell == c)
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// The knowledge after an observation made at `at`. A sensor that missed its marker adds
    /// nothing but the record that it failed.
    pub fn observed(&self, obs: &Observation, at: u64) -> Knowledge {
        let mut k = self.clone();
        k.step = at;
        k.awaiting = None;
        if !obs.marker_seen {
            k.senses_failed_at = Some(at);
            return k;
        }
        k.senses_ok_at = Some(at);
        for (c, t) in &obs.terrain {
            k.terrain.insert(key(*c), t.clone());
        }
        for c in &obs.seen {
            let looks = k.looks.entry(key(*c)).or_default();
            looks.push(Look {
                at,
                resolution_mm: obs.resolution_mm,
            });
            if looks.len() > MAX_LOOKS {
                looks.remove(0);
            }
            // Whoever was here before is wherever this look found them now.
            k.beings.retain(|b| b.cell != *c);
        }
        for b in &obs.beings {
            k.beings.retain(|x| x.id != b.id);
            k.beings.push(b.clone());
        }
        k.beings.sort_by(|a, b| a.id.cmp(&b.id));
        k
    }

    /// The knowledge after an allowed act, as the act is expected to leave it at step `at`. What
    /// it actually did is only known by looking again.
    pub fn after(&self, act: &Act, at: u64) -> Knowledge {
        let mut k = self.clone();
        k.step = at;
        match act {
            Act::Move { path } | Act::Drive { path, .. } => {
                if let Some(last) = path.last() {
                    k.body = *last;
                }
            }
            Act::RemovePanel { wall } => {
                if let Some(Terrain::Wall { panel_on, .. }) = k.terrain.get_mut(&key(*wall)) {
                    *panel_on = false;
                }
            }
            Act::Strike { target, depth_mm } => {
                if let Some(Terrain::Wall { thickness_mm, .. }) = k.terrain.get(&key(*target)) {
                    if depth_mm >= thickness_mm {
                        k.terrain.insert(key(*target), Terrain::Open);
                    } else {
                        let left = thickness_mm - depth_mm;
                        if let Some(Terrain::Wall { thickness_mm, .. }) =
                            k.terrain.get_mut(&key(*target))
                        {
                            *thickness_mm = left;
                        }
                    }
                }
                // The struck cell is not as it was seen.
                k.looks.remove(&key(*target));
            }
            Act::Lock { door } | Act::Unlock { door } => {
                let locked = matches!(act, Act::Lock { .. });
                if let Some(Terrain::Door { locked: l }) = k.terrain.get_mut(&key(*door)) {
                    *l = locked;
                }
            }
            Act::Observe { cells } => k.awaiting = Some(cells.clone()),
            Act::Speak { .. } | Act::Wait {} | Act::AimWeapon { .. } | Act::FireWeapon { .. } => {}
        }
        k
    }
}

pub fn neighbours(c: Cell) -> [Cell; 4] {
    [
        (c.0 + 1, c.1),
        (c.0 - 1, c.1),
        (c.0, c.1 + 1),
        (c.0, c.1 - 1),
    ]
}

pub fn adjacent(a: Cell, b: Cell) -> bool {
    (a.0 - b.0).abs() + (a.1 - b.1).abs() == 1
}

impl Knowledge {
    /// Whether an observation answers what this knowledge is waiting for, and no more: it saw
    /// only cells it was asked to look at, and reports beings only where it saw.
    pub fn answers(&self, obs: &Observation) -> bool {
        let Some(asked) = &self.awaiting else {
            return false;
        };
        obs.resolution_mm > 0
            && obs.seen.len() <= asked.len()
            && obs.seen.iter().all(|c| asked.contains(c))
            && obs.terrain.iter().all(|(c, _)| asked.contains(c))
            && obs.beings.iter().all(|b| obs.seen.contains(&b.cell))
            && (obs.marker_seen
                || (obs.seen.is_empty() && obs.terrain.is_empty() && obs.beings.is_empty()))
    }
}
