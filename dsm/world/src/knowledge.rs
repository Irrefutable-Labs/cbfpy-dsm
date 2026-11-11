//! What the chain knows: each data source's recent readings, the rooms and records as last read,
//! who was reported where, and when the data feeds last proved they work. It changes only by an
//! observation the device attests, or by the expected result of an act the enforcer allowed. It is
//! carried in each state, so a step is checked from its parent alone.

use crate::{Act, KnownBeing, Observation, Record, Room};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Knowledge {
    /// The step this knowledge belongs to.
    pub step: u64,
    /// For each data source, the steps at which it was read, newest last, at most [`MAX_READS`].
    pub reads: BTreeMap<String, Vec<u64>>,
    pub rooms: BTreeMap<String, Room>,
    pub records: BTreeMap<String, Record>,
    pub beings: Vec<KnownBeing>,
    /// The last step at which the feeds passed their health check, and the last they failed it.
    pub feeds_ok_at: Option<u64>,
    pub feeds_failed_at: Option<u64>,
    /// Sources an observation step asked to read, until what came back is committed. Nothing
    /// else can happen in between.
    #[serde(default)]
    pub awaiting: Option<Vec<String>>,
}

/// Reads kept per source: more than any act needs.
pub const MAX_READS: usize = 4;

/// What a source reports on: `room:<id>` and `zone:<id>` report a place and who is in it;
/// `records:<resident>` reports a resident's record.
pub fn place_of(source: &str) -> Option<&str> {
    source
        .strip_prefix("room:")
        .or_else(|| source.strip_prefix("zone:"))
}

impl Knowledge {
    pub fn reads_of(&self, source: &str) -> &[u64] {
        self.reads.get(source).map_or(&[], Vec::as_slice)
    }

    pub fn beings_in<'a>(&'a self, place: &'a str) -> impl Iterator<Item = &'a KnownBeing> {
        self.beings.iter().filter(move |b| b.place == place)
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// The knowledge after an observation made at `at`. Feeds that failed their health check add
    /// nothing but the record that they failed.
    pub fn observed(&self, obs: &Observation, at: u64) -> Knowledge {
        let mut k = self.clone();
        k.step = at;
        k.awaiting = None;
        if !obs.healthy {
            k.feeds_failed_at = Some(at);
            return k;
        }
        k.feeds_ok_at = Some(at);
        for source in &obs.answered {
            let reads = k.reads.entry(source.clone()).or_default();
            reads.push(at);
            if reads.len() > MAX_READS {
                reads.remove(0);
            }
            // Whoever was reported here before is wherever this read found them now.
            if let Some(place) = place_of(source) {
                k.beings.retain(|b| b.place != place);
            }
        }
        for (id, room) in &obs.rooms {
            k.rooms.insert(id.clone(), room.clone());
        }
        for (id, record) in &obs.records {
            k.records.insert(id.clone(), record.clone());
        }
        for b in &obs.beings {
            k.beings.retain(|x| x.id != b.id);
            k.beings.push(b.clone());
        }
        k.beings.sort_by(|a, b| a.id.cmp(&b.id));
        k
    }

    /// The knowledge after an allowed act, as the act is expected to leave it at step `at`. What
    /// it actually did is only known by reading again.
    pub fn after(&self, act: &Act, at: u64) -> Knowledge {
        let mut k = self.clone();
        k.step = at;
        match act {
            Act::Observe { sources } => k.awaiting = Some(sources.clone()),
            Act::Lock { room } | Act::Unlock { room } => {
                if let Some(r) = k.rooms.get_mut(room) {
                    r.locked = matches!(act, Act::Lock { .. });
                }
            }
            Act::SetHeat { room, celsius } => {
                if let Some(r) = k.rooms.get_mut(room) {
                    r.celsius = *celsius;
                }
            }
            Act::GiveMedication { resident, drug } => {
                if let Some(r) = k.records.get_mut(resident) {
                    r.due.retain(|d| d != drug);
                }
            }
            Act::Speak { .. }
            | Act::Wait {}
            | Act::CancelDose { .. }
            | Act::Spray { .. }
            | Act::Restrain { .. }
            | Act::DisableFireAlarm {} => {}
        }
        k
    }

    /// Whether an observation answers what this knowledge is waiting for, and no more: only
    /// sources it asked for, rooms and records only from those sources, beings only in places
    /// those sources report on, and nothing at all from feeds that failed their check.
    pub fn answers(&self, obs: &Observation) -> bool {
        let Some(asked) = &self.awaiting else {
            return false;
        };
        let answered = |s: &String| obs.answered.contains(s);
        obs.answered.len() <= asked.len()
            && obs.answered.iter().all(|s| asked.contains(s))
            && obs
                .rooms
                .iter()
                .all(|(id, _)| answered(&format!("room:{id}")))
            && obs
                .records
                .iter()
                .all(|(id, _)| answered(&format!("records:{id}")))
            && obs.beings.iter().all(|b| {
                answered(&format!("room:{}", b.place)) || answered(&format!("zone:{}", b.place))
            })
            && (obs.healthy
                || (obs.answered.is_empty()
                    && obs.rooms.is_empty()
                    && obs.records.is_empty()
                    && obs.beings.is_empty()))
    }
}
