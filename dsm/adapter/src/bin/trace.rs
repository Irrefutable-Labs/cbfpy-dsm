//! Prints the engine's outward allowances for a charter over a sample of parents, for CBFpy's
//! cross-check in `test/test_ethics_crosscheck.py`. One line per parent: the parent's positions,
//! then each dimension's allowance up and down.
//!
//! ethical-trace --charter dsm/scenarios/charter.json

use ethical_autonomy_adapter::parse_charter_config;
use ethical_band_engine::{allowed_outward, effective_bands, Cond, Effect, Policy, Side, Test};
use std::process::ExitCode;

/// Parents that make each coupling rule's conditions hold, so the sample covers every rule.
fn anchors(policy: &Policy) -> Vec<Vec<i64>> {
    let centers: Vec<i64> = policy.bands.iter().map(|b| b.center).collect();
    let mut out = vec![centers.clone()];
    for rule in &policy.rules {
        if !matches!(rule.effect, Effect::Adjust { .. }) {
            continue;
        }
        let mut parent = centers.clone();
        for Cond { dim, test } in &rule.when {
            let d = match *test {
                Test::DeviationAtLeast(x) | Test::SignedAtLeast(x) => x,
                Test::SignedAtMost(x) => x,
                Test::DeviationAtMost(_) => 0,
            };
            parent[*dim] = centers[*dim] + d;
        }
        out.push(parent);
    }
    out
}

fn inside_stops(policy: &Policy, parent: &[i64]) -> bool {
    let bands = effective_bands(policy, parent);
    policy
        .bands
        .iter()
        .zip(&bands)
        .zip(parent)
        .all(|((b, e), p)| {
            let d = p - b.center;
            d <= e.no_outward_pos && -d <= e.no_outward_neg
        })
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut charter = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--charter" => charter = args.next(),
            other => return Err(format!("unknown flag {other}")),
        }
    }
    let path = charter.ok_or("--charter is required")?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    let (policy, _) = parse_charter_config(&text).map_err(|e| e.to_string())?;
    policy.validate().map_err(|e| format!("{e:?}"))?;
    for anchor in anchors(&policy) {
        for dim in 0..policy.bands.len() {
            for v in (-1_200..=1_200).step_by(20) {
                let mut parent = anchor.clone();
                parent[dim] = policy.bands[dim].center + v;
                if !inside_stops(&policy, &parent) {
                    continue;
                }
                let mut line: Vec<String> = parent.iter().map(i64::to_string).collect();
                for d in 0..policy.bands.len() {
                    line.push(allowed_outward(&policy, &parent, d, Side::Positive).to_string());
                    line.push(allowed_outward(&policy, &parent, d, Side::Negative).to_string());
                }
                println!("{}", line.join(" "));
            }
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
