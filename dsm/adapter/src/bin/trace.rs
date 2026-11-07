//! The van's speed limit as the enforcer decides it, for the CBFpy cross-check in
//! `test/test_ethics_crosscheck.py`. One line per case: the speed in cm/s, the cells of road seen
//! clear ahead of where the van will be, and 1 if the drive is allowed or 0 if not.
//!
//! ethical-trace --charter scenarios/charter.json
//!
//! Every verdict comes from `ethical_world::check`, on knowledge built as a run would commit it:
//! the road looked at twice, recently, at a resolution fine enough to see a honeybee.

use ethical_autonomy_adapter::parse_charter_config;
use ethical_world::check::check;
use ethical_world::knowledge::Knowledge;
use ethical_world::{Act, Cell, Observation, Terrain};
use std::process::ExitCode;

fn road(seen_ahead: i32, resolution_mm: u32) -> Knowledge {
    let cells: Vec<Cell> = (1..=1 + seen_ahead).map(|x| (x, 0)).collect();
    let terrain: Vec<(Cell, Terrain)> = (0..=60).map(|x| ((x, 0), Terrain::Road)).collect();
    let look = Observation {
        seen: cells,
        resolution_mm,
        marker_seen: true,
        terrain,
        beings: vec![],
    };
    let k = Knowledge::starting_at((0, 0));
    let k = k
        .after(&Act::Observe { cells: vec![] }, 1)
        .observed(&look, 2);
    k.after(&Act::Observe { cells: vec![] }, 3)
        .observed(&look, 4)
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
    let fine = policy.foundation.smallest_protected_mm();
    for seen_ahead in 0..=25 {
        let k = road(seen_ahead, fine);
        for speed in (50..=policy.operational.top_speed_cm_s).step_by(50) {
            let v = check(
                &policy.foundation,
                &policy.operational,
                &k,
                &Act::Drive {
                    path: vec![(1, 0)],
                    speed_cm_s: speed,
                },
            );
            println!("{speed} {seen_ahead} {}", u8::from(v.allowed()));
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
