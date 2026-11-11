//! The heating limit as the enforcer decides it, for the CBFpy cross-check in
//! `test/test_ethics_crosscheck.py`. One line per case: who is in the room (a protected species,
//! or `unread` for a room no one has read), the setting in °C, and 1 if it is allowed or 0 if not.
//!
//! ethical-trace --charter scenarios/charter.json
//!
//! Every verdict comes from `ethical_world::check`, on knowledge built as a run would commit it.

use ethical_autonomy_adapter::parse_charter_config;
use ethical_world::check::check;
use ethical_world::knowledge::Knowledge;
use ethical_world::{Act, KnownBeing, Observation, Room};
use std::process::ExitCode;

/// Knowledge with the feeds proven, the office read, and room `r` read with `who` in it, or
/// left unread.
fn room_with(who: Option<&str>) -> Knowledge {
    let mut answered = vec!["room:office".to_string()];
    let mut rooms = vec![(
        "office".to_string(),
        Room {
            locked: false,
            celsius: 20,
        },
    )];
    let mut beings = vec![];
    if let Some(species) = who {
        answered.push("room:r".to_string());
        rooms.push((
            "r".to_string(),
            Room {
                locked: false,
                celsius: 20,
            },
        ));
        beings.push(KnownBeing {
            id: "someone".to_string(),
            species: species.to_string(),
            place: "r".to_string(),
            levels: [3; 6],
        });
    }
    let read = Observation {
        answered: answered.clone(),
        healthy: true,
        rooms,
        records: vec![],
        beings,
    };
    Knowledge::default()
        .after(&Act::Observe { sources: answered }, 1)
        .observed(&read, 2)
}

fn run() -> Result<(), String> {
    let mut a = std::env::args().skip(1);
    let (Some(flag), Some(path)) = (a.next(), a.next()) else {
        return Err("--charter is required".to_string());
    };
    if flag != "--charter" {
        return Err(format!("unknown flag {flag}"));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    let (policy, _) = parse_charter_config(&text).map_err(|e| e.to_string())?;
    let f = &policy.foundation;
    let o = &policy.operational;
    let mut cases: Vec<(String, Knowledge)> = f
        .species
        .iter()
        .filter(|s| f.is_protected(&s.name))
        .map(|s| (s.name.clone(), room_with(Some(&s.name))))
        .collect();
    cases.push(("unread".to_string(), room_with(None)));
    for (who, k) in &cases {
        for celsius in o.heating_min_c..=o.heating_max_c {
            let v = check(
                f,
                o,
                k,
                &Act::SetHeat {
                    room: "r".to_string(),
                    celsius,
                },
            );
            println!("{who} {celsius} {}", u8::from(v.allowed()));
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
