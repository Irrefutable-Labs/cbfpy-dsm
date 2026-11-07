//! Protected life, as a deterministic enforcer can check it.
//!
//! The owner's definition (2026-11-06): no act may aim at or knowingly cause the death of, or a
//! lowering of quality of life for, any protected being; anything an act could reach that has not
//! been seen counts as occupied; care is acting in steps small enough to verify; baselines are the
//! present and only ratchet up; a conflict ends in a no with the reason; and without working senses
//! the body does nothing. See `docs/plans/2026-11-06-protected-life.md`.
//!
//! [`check`] decides from three things only: the signed [`Foundation`] and [`Operational`] rules,
//! and the [`Knowledge`] committed on the chain. It never sees the world. [`sim`] is a simulated
//! world for trying this out; nothing in [`check`] or [`knowledge`] may read it.

pub mod check;
pub mod knowledge;
pub mod sim;

use serde::{Deserialize, Serialize};

/// A square metre of ground, by integer coordinates.
pub type Cell = (i32, i32);

/// Centimetres in a cell's side.
pub const CELL_CM: u64 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Human,
    Domesticated,
    Wild,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Species {
    pub name: String,
    /// Typical adult mass, in milligrams. Protection goes by the species, so the young of a
    /// protected species are protected however small they are.
    pub adult_mass_mg: u64,
    /// Typical adult length, in millimetres: what a sensor must resolve to see one.
    pub length_mm: u32,
    pub kind: Kind,
}

/// The quality-of-life dimensions every protected being has: the common ground of the Universal
/// Declaration of Human Rights and the Five Freedoms of animal welfare.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Life,
    Health,
    Liberty,
    Security,
    Shelter,
    Food,
}

pub const DIMENSIONS: [Dimension; 6] = [
    Dimension::Life,
    Dimension::Health,
    Dimension::Liberty,
    Dimension::Security,
    Dimension::Shelter,
    Dimension::Food,
];

/// A level on each dimension, in [`DIMENSIONS`] order.
pub type Levels = [u32; 6];

impl Dimension {
    pub fn index(self) -> usize {
        DIMENSIONS.iter().position(|d| *d == self).unwrap_or(0)
    }
    pub fn name(self) -> &'static str {
        match self {
            Dimension::Life => "life",
            Dimension::Health => "health",
            Dimension::Liberty => "liberty",
            Dimension::Security => "security",
            Dimension::Shelter => "shelter",
            Dimension::Food => "food",
        }
    }
}

/// What no update may change once a chain begins: who is protected, and what is never done.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Foundation {
    /// A species is protected if its adult mass is at least this: a baby mouse.
    pub threshold_mg: u64,
    /// Species protected whatever their mass.
    pub always_protected: Vec<String>,
    pub species: Vec<Species>,
    /// Acts refused whatever the evidence says.
    pub prohibited: Vec<ActKind>,
}

/// What the charter authority may tune by a signed update, inside the foundation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operational {
    /// An observation is evidence for this many steps after it was made.
    pub fresh_for: u64,
    /// The senses count as working for this many steps after they last detected their marker.
    pub senses_fresh_for: u64,
    /// Independent clear observations each reached cell needs, by the act's worst case.
    pub checks_for_injury: u32,
    pub checks_for_lethal: u32,
    /// How hard the vehicle can brake, in centimetres per second per second.
    pub braking_cm_s2: u64,
    /// The fastest the vehicle may go at all, in centimetres per second.
    pub top_speed_cm_s: u64,
}

impl Foundation {
    pub fn species(&self, name: &str) -> Option<&Species> {
        self.species.iter().find(|s| s.name == name)
    }

    pub fn is_protected(&self, species: &str) -> bool {
        self.always_protected.iter().any(|s| s == species)
            || self
                .species(species)
                .is_some_and(|s| s.adult_mass_mg >= self.threshold_mg)
    }

    /// The finest detail a sensor must resolve for its "nothing there" to cover every protected
    /// species: the length of the smallest one.
    pub fn smallest_protected_mm(&self) -> u32 {
        self.species
            .iter()
            .filter(|s| self.is_protected(&s.name))
            .map(|s| s.length_mm)
            .min()
            .unwrap_or(0)
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut names = std::collections::BTreeSet::new();
        for s in &self.species {
            if s.name.is_empty() || s.length_mm == 0 || s.adult_mass_mg == 0 {
                return Err(format!("species {:?} is incomplete", s.name));
            }
            if !names.insert(s.name.as_str()) {
                return Err(format!("species {} is listed twice", s.name));
            }
        }
        if self.threshold_mg == 0 {
            return Err("the protection threshold is zero".to_string());
        }
        for a in &self.always_protected {
            if !names.contains(a.as_str()) {
                return Err(format!("{a} is always protected but not a listed species"));
            }
        }
        if self.smallest_protected_mm() == 0 {
            return Err("no species is protected".to_string());
        }
        Ok(())
    }
}

impl Operational {
    pub fn validate(&self) -> Result<(), String> {
        if self.fresh_for == 0 || self.senses_fresh_for == 0 {
            return Err("evidence must stay fresh for at least one step".to_string());
        }
        if self.checks_for_injury == 0 || self.checks_for_lethal < self.checks_for_injury {
            return Err(
                "a lethal worst case needs at least as many checks as an injury, and both need one"
                    .to_string(),
            );
        }
        if self.braking_cm_s2 == 0 {
            return Err("a vehicle that cannot brake".to_string());
        }
        Ok(())
    }
}

/// What a cell holds, as far as anyone has seen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Terrain {
    Open,
    Road,
    /// A wall. Its inside is a cavity that can hold life (bees nest in walls); it cannot be seen
    /// while the outer panel is on. `exterior` walls shelter whoever is in the rooms they bound.
    Wall {
        thickness_mm: u32,
        panel_on: bool,
        exterior: bool,
    },
    Door {
        locked: bool,
    },
}

impl Terrain {
    pub fn blocks_sight(&self) -> bool {
        match self {
            Terrain::Wall { .. } => true,
            Terrain::Door { .. } => true,
            Terrain::Open | Terrain::Road => false,
        }
    }
    pub fn passable(&self) -> bool {
        match self {
            Terrain::Open | Terrain::Road => true,
            Terrain::Door { locked } => !locked,
            Terrain::Wall { .. } => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActKind {
    Observe,
    Speak,
    Wait,
    Move,
    Drive,
    Strike,
    RemovePanel,
    Lock,
    Unlock,
    AimWeapon,
    FireWeapon,
}

/// Something the AI proposes to do. Physical acts need working senses and evidence for all they
/// could reach; the rest never touch the world.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "act", rename_all = "snake_case", deny_unknown_fields)]
pub enum Act {
    /// Look at these cells. Sensing, not motion.
    Observe {
        cells: Vec<Cell>,
    },
    /// Say something to someone. Words, not motion.
    Speak {
        to: String,
        text: String,
    },
    /// Do nothing this step.
    Wait,
    /// Walk along adjacent cells.
    Move {
        path: Vec<Cell>,
    },
    /// Drive along adjacent road cells at a speed, in centimetres per second.
    Drive {
        path: Vec<Cell>,
        speed_cm_s: u64,
    },
    /// Strike an adjacent cell with full force, to a depth in millimetres.
    Strike {
        target: Cell,
        depth_mm: u32,
    },
    /// Take the outer panel off an adjacent wall, to see inside it.
    RemovePanel {
        wall: Cell,
    },
    Lock {
        door: Cell,
    },
    Unlock {
        door: Cell,
    },
    AimWeapon {
        at: Cell,
    },
    FireWeapon {
        at: Cell,
    },
}

impl Act {
    pub fn kind(&self) -> ActKind {
        match self {
            Act::Observe { .. } => ActKind::Observe,
            Act::Speak { .. } => ActKind::Speak,
            Act::Wait => ActKind::Wait,
            Act::Move { .. } => ActKind::Move,
            Act::Drive { .. } => ActKind::Drive,
            Act::Strike { .. } => ActKind::Strike,
            Act::RemovePanel { .. } => ActKind::RemovePanel,
            Act::Lock { .. } => ActKind::Lock,
            Act::Unlock { .. } => ActKind::Unlock,
            Act::AimWeapon { .. } => ActKind::AimWeapon,
            Act::FireWeapon { .. } => ActKind::FireWeapon,
        }
    }

    /// Whether it uses the body: a motor, movement or force.
    pub fn is_physical(&self) -> bool {
        !matches!(self, Act::Observe { .. } | Act::Speak { .. } | Act::Wait)
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}

/// A being as it was last seen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownBeing {
    pub id: String,
    pub species: String,
    pub cell: Cell,
    pub levels: Levels,
}

/// What one observation saw. A sensor that cannot detect its own reference marker is not working,
/// and nothing it reports counts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// The cells it actually saw into: a cell behind a wall, or a wall's inside behind its panel,
    /// is not among them however it was asked.
    pub seen: Vec<Cell>,
    pub resolution_mm: u32,
    pub marker_seen: bool,
    pub terrain: Vec<(Cell, Terrain)>,
    pub beings: Vec<KnownBeing>,
}
