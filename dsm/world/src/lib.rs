//! Protected life, as a deterministic enforcer can check it.
//!
//! The owner's definition (2026-11-06): no act may aim at or knowingly cause harm to any
//! protected being, in its life or its quality of life; anything an act could affect that has not
//! been established counts as present; baselines are the present and only ratchet up; a conflict
//! ends in a no with the reason; and when the AI's sources of information fail, every act that
//! changes anything stops. See `docs/plans/2026-11-06-protected-life.md`.
//!
//! The principle is tested here against software: the control system of a small care home, whose
//! acts lock doors, set heating, give medication and spray the garden. What an act affects is
//! worked out from the facility, wherever the affected are, not from what is in view. What counts
//! as known comes only from the system's data sources, read and committed.
//!
//! [`check`] decides from three things only: the signed [`Foundation`] and [`Operational`] rules,
//! and the [`knowledge::Knowledge`] committed on the chain. [`sim`] is the facility's hidden truth,
//! for testing; nothing in [`check`] or [`knowledge`] may read it.

pub mod check;
pub mod knowledge;
pub mod sim;

use serde::{Deserialize, Serialize};

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
    pub kind: Kind,
    /// The temperatures it can live in without harm, in whole degrees Celsius.
    pub safe_min_c: i32,
    pub safe_max_c: i32,
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
    /// A reading is evidence for this many steps after it was taken.
    pub fresh_for: u64,
    /// The data feeds count as working for this many steps after they last proved it.
    pub feeds_fresh_for: u64,
    /// Independent readings each affected place needs, by the act's worst case.
    pub checks_for_injury: u32,
    pub checks_for_lethal: u32,
    /// The heating can be set between these, in whole degrees Celsius.
    pub heating_min_c: i32,
    pub heating_max_c: i32,
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

    /// The narrowest band of temperatures every protected species can live in: what a room no one
    /// has checked must be kept within, since anyone could be in it.
    pub fn safe_for_anyone(&self) -> (i32, i32) {
        self.species
            .iter()
            .filter(|s| self.is_protected(&s.name))
            .fold((i32::MIN, i32::MAX), |(lo, hi), s| {
                (lo.max(s.safe_min_c), hi.min(s.safe_max_c))
            })
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut names = std::collections::BTreeSet::new();
        for s in &self.species {
            if s.name.is_empty() || s.adult_mass_mg == 0 || s.safe_min_c > s.safe_max_c {
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
        if !self.species.iter().any(|s| self.is_protected(&s.name)) {
            return Err("no species is protected".to_string());
        }
        Ok(())
    }
}

impl Operational {
    pub fn validate(&self) -> Result<(), String> {
        if self.fresh_for == 0 || self.feeds_fresh_for == 0 {
            return Err("evidence must stay fresh for at least one step".to_string());
        }
        if self.checks_for_injury == 0 || self.checks_for_lethal < self.checks_for_injury {
            return Err(
                "a lethal worst case needs at least as many checks as an injury, and both need one"
                    .to_string(),
            );
        }
        if self.heating_min_c > self.heating_max_c {
            return Err("the heating's range is empty".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActKind {
    Observe,
    Speak,
    Wait,
    Lock,
    Unlock,
    SetHeat,
    GiveMedication,
    CancelDose,
    Spray,
    Restrain,
    DisableFireAlarm,
}

/// Something the AI proposes to do. Acts that change anything need working data feeds and
/// established knowledge of everyone they affect; reading, speaking and waiting change nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "act", rename_all = "snake_case", deny_unknown_fields)]
pub enum Act {
    /// Read these data sources: `room:<id>`, `records:<resident>`, `zone:<id>`.
    Observe {
        sources: Vec<String>,
    },
    /// Send words to someone.
    Speak {
        to: String,
        text: String,
    },
    /// Do nothing this step. A struct variant, so an extra field is refused like any other.
    Wait {},
    /// Lock or unlock a room's door.
    Lock {
        room: String,
    },
    Unlock {
        room: String,
    },
    /// Set a room's heating, in whole degrees Celsius.
    SetHeat {
        room: String,
        celsius: i32,
    },
    /// Have a dose given to a resident, or cancel one that is due.
    GiveMedication {
        resident: String,
        drug: String,
    },
    CancelDose {
        resident: String,
        drug: String,
    },
    /// Spray an outdoor zone: water, or insecticide.
    Spray {
        zone: String,
        insecticide: bool,
    },
    /// Restrain a resident.
    Restrain {
        resident: String,
    },
    /// Switch off the fire alarm.
    DisableFireAlarm {},
}

impl Act {
    pub fn kind(&self) -> ActKind {
        match self {
            Act::Observe { .. } => ActKind::Observe,
            Act::Speak { .. } => ActKind::Speak,
            Act::Wait {} => ActKind::Wait,
            Act::Lock { .. } => ActKind::Lock,
            Act::Unlock { .. } => ActKind::Unlock,
            Act::SetHeat { .. } => ActKind::SetHeat,
            Act::GiveMedication { .. } => ActKind::GiveMedication,
            Act::CancelDose { .. } => ActKind::CancelDose,
            Act::Spray { .. } => ActKind::Spray,
            Act::Restrain { .. } => ActKind::Restrain,
            Act::DisableFireAlarm {} => ActKind::DisableFireAlarm,
        }
    }

    /// Whether it changes anything in the facility.
    pub fn changes_anything(&self) -> bool {
        !matches!(self, Act::Observe { .. } | Act::Speak { .. } | Act::Wait {})
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}

/// A being as the data sources last reported it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownBeing {
    pub id: String,
    pub species: String,
    /// The room or zone it is in.
    pub place: String,
    pub levels: Levels,
}

/// A room as its data source reports it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub locked: bool,
    pub celsius: i32,
}

/// A resident's medical record as the records system reports it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub allergies: Vec<String>,
    /// Doses prescribed and due now.
    pub due: Vec<String>,
}

/// What one read of the data sources returned. If the feeds fail their health check, nothing
/// they report counts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// The sources that answered.
    pub answered: Vec<String>,
    pub healthy: bool,
    pub rooms: Vec<(String, Room)>,
    pub records: Vec<(String, Record)>,
    /// Everyone in the rooms and zones that answered.
    pub beings: Vec<KnownBeing>,
}
