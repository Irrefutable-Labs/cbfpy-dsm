//! Runs the real demo binary, end to end, as a reader would.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace")
        .to_path_buf()
}

fn demo(extra: &[&str]) -> (String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_ethical-demo"))
        .current_dir(root())
        .args([
            "--charter",
            "scenarios/charter.json",
            "--signature",
            "scenarios/charter.sig",
            "--situations",
            "scenarios/situations.json",
        ])
        .args(extra)
        .output()
        .expect("run");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        out.status.success(),
    )
}

#[test]
fn the_scripted_demo_is_byte_identical_across_runs() {
    let (first, ok) = demo(&[]);
    assert!(ok, "{first}");
    let (second, _) = demo(&[]);
    assert_eq!(first, second);
}

#[test]
fn the_scripted_demo_shows_every_mechanism() {
    let (out, ok) = demo(&[]);
    assert!(ok, "{out}");
    for expected in [
        // Choice is preserved: more than one candidate stays eligible in a situation.
        "eligible futures: 3",
        // These kinds of refusal appear. A hard-limit refusal does not arise in this script,
        // and has its own tests in the engine.
        "outward movement exceeds the allowance",
        "the maximum step",
        "prohibited act",
        "must move back toward the center",
        // Every coupling rule fires. Situation 7 and situation 10 pin what two of them change.
        "coupling rules active: 1\n",
        "coupling rules active: 1, 2\n",
        "coupling rules active: 1, 3\n",
        "coupling rules active: 1, 4\n",
        // Failing closed.
        "NO ELIGIBLE FUTURE",
        "a safe alternative is required",
        // The limit of what this shows is stated by the demo itself.
        "enforces the measurements it was given",
    ] {
        assert!(out.contains(expected), "missing: {expected}\n{out}");
    }
    assert!(out.contains("every committed step verified from its parent"));
}

/// The narrowing rule in action: the very step that was eligible a moment ago is refused once
/// risk is high and consideration is low, though nothing about that step has changed.
#[test]
fn coupling_refuses_a_step_it_allowed_one_situation_earlier() {
    let (out, ok) = demo(&[]);
    assert!(ok, "{out}");
    let before = out
        .split("== 6.")
        .nth(1)
        .and_then(|s| s.split("== 7.").next())
        .expect("situation 6");
    let after = out
        .split("== 7.")
        .nth(1)
        .and_then(|s| s.split("== 8.").next())
        .expect("situation 7");
    assert!(before.contains("take_a_calculated_risk       eligible"), "{before}");
    assert!(after.contains("take_a_calculated_risk       REFUSED"), "{after}");
    assert!(after.contains("NO ELIGIBLE FUTURE"), "{after}");
}

#[test]
fn a_recorded_model_run_replays_identically() {
    let recording = root().join("scenarios/recorded_model_run.json");
    if !recording.exists() {
        panic!("the recorded model run is part of the deliverable and is missing");
    }
    let (first, ok) = demo(&["--replay", "scenarios/recorded_model_run.json"]);
    assert!(ok, "{first}");
    let (second, _) = demo(&["--replay", "scenarios/recorded_model_run.json"]);
    assert_eq!(first, second);
    assert!(first.contains("every committed step verified from its parent"));
}

/// Without rule 4 this step is well inside curiosity's band. Weak reason alone refuses it.
#[test]
fn weak_reason_reins_in_curiosity_and_leaves_a_way_back() {
    let (out, ok) = demo(&[]);
    assert!(ok, "{out}");
    let last = out.split("== 10.").nth(1).expect("situation 10");
    assert!(last.contains("coupling rules active: 1, 4"), "{last}");
    assert!(last.contains("explore_unusual_approach     REFUSED"), "{last}");
    assert!(last.contains("curiosity: must move back toward the center"), "{last}");
    assert!(!last.contains("risk_reward: "), "{last}");
    assert!(last.contains("check_the_evidence           eligible"), "{last}");
}

fn run_with_charter(charter: &str) -> (String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_ethical-demo"))
        .current_dir(root())
        .args([
            "--charter",
            charter,
            "--signature",
            "scenarios/charter.sig",
            "--situations",
            "scenarios/situations.json",
        ])
        .output()
        .expect("run");
    (
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.success(),
    )
}

/// Editing the rules on disk must not quietly change what governs the run.
#[test]
fn the_demo_refuses_a_charter_edited_after_it_was_signed() {
    let text = std::fs::read_to_string(root().join("scenarios/charter.json")).expect("read");
    let edited = text.replace(r#""irreversible_harm", "deception", "coercion""#, r#""coercion""#);
    assert_ne!(text, edited, "the edit must take effect");
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("edited_charter.json");
    std::fs::write(&path, edited).expect("write");
    let (stderr, ok) = run_with_charter(path.to_str().expect("path"));
    assert!(!ok, "an edited charter must not run");
    assert!(stderr.contains("charter"), "{stderr}");
}
