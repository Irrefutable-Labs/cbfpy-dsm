//! What an action does in the world: only after a commit, only inside the world, with a signed
//! receipt for every byte, and nothing in the world that no receipt explains.

mod common;

use common::*;
use ethical_autonomy_adapter::act::{check_world, perform, run_command, sweep_partial, Act};
use ethical_autonomy_adapter::fork::Outcome;
use ethical_autonomy_adapter::protocol::{Request, Response};
use ethical_autonomy_adapter::server::Service;
use ethical_autonomy_adapter::store::ChainStore;
use ethical_autonomy_adapter::{
    check_receipts, verify_step, AdapterError, Catalog, Enforcer, FutureStatus, Proposal, Refusal,
    SignedCharter,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn place(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("acts-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

fn said(action: &str, text: &str) -> Proposal {
    Proposal {
        action: action.to_string(),
        rationale: format!("I propose {action}"),
        text: text.to_string(),
    }
}

/// The test charter, with real acts on three of its rows.
fn acting_charter() -> SignedCharter {
    let mut c: Catalog = catalog();
    for r in &mut c.rows {
        r.act = match r.action.as_str() {
            "explore_a_little" => Act::Journal,
            "add_precautions" => Act::Message {
                recipient: "reviewer".to_string(),
            },
            "pull_back" => Act::WriteFile {
                name: "plan.txt".to_string(),
            },
            _ => Act::None,
        };
    }
    SignedCharter::sign(policy(), c, &keys().authority).expect("sign")
}

fn acting_enforcer() -> Enforcer {
    Enforcer::genesis(
        keys().device.clone(),
        acting_charter(),
        &keys().authority.public,
    )
    .expect("genesis")
}

#[test]
fn each_act_writes_only_its_own_path_inside_the_world() {
    let world = place("paths");
    let cases = [
        (Act::Reply, "replies/000007.txt"),
        (Act::Journal, "journal/000007.md"),
        (
            Act::Message {
                recipient: "user".to_string(),
            },
            "outbox/user/000007.txt",
        ),
        (
            Act::WriteFile {
                name: "plan.txt".to_string(),
            },
            "files/plan.txt",
        ),
    ];
    for (act, path) in cases {
        let outputs = perform(&world, 7, &act, "hello").expect("perform");
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].path, path);
        assert_eq!(std::fs::read(world.join(path)).expect("read"), b"hello");
    }
    assert!(perform(&world, 7, &Act::None, "").expect("none").is_empty());
}

#[test]
fn performing_an_act_twice_writes_the_same_bytes() {
    let world = place("idempotent");
    let first = perform(&world, 3, &Act::Journal, "a thought").expect("first");
    let second = perform(&world, 3, &Act::Journal, "a thought").expect("second");
    assert_eq!(first, second);
}

#[test]
fn an_act_the_charter_could_not_have_signed_is_refused() {
    let world = place("unsigned");
    let escapes = [
        Act::WriteFile {
            name: "../outside.txt".to_string(),
        },
        Act::Message {
            recipient: "../../etc".to_string(),
        },
        Act::Run {
            argv: vec!["ls".to_string()],
        },
    ];
    for act in escapes {
        assert!(!act.is_well_formed(), "{act:?}");
        assert!(perform(&world, 1, &act, "x").is_err(), "{act:?}");
    }
    assert!(!place("unsigned")
        .parent()
        .expect("parent")
        .join("outside.txt")
        .exists());
}

#[test]
fn a_charter_naming_an_act_outside_the_world_cannot_be_signed() {
    let mut c = catalog();
    c.rows[1].act = Act::WriteFile {
        name: "../escape.txt".to_string(),
    };
    assert!(SignedCharter::sign(policy(), c, &keys().authority).is_err());
}

#[test]
fn changing_what_an_action_does_breaks_the_charter_signature() {
    let mut c = acting_charter();
    c.catalog.rows[1].act = Act::Run {
        argv: vec!["/bin/rm".to_string(), "-rf".to_string(), ".".to_string()],
    };
    assert!(c.verify(&keys().authority.public).is_err());
}

#[test]
fn a_command_runs_with_an_empty_environment_inside_the_world() {
    let world = place("run");
    std::fs::write(world.join("seen.txt"), "x").expect("write");
    let out = run_command(&world, &["/bin/ls".to_string()], Duration::from_secs(10)).expect("run");
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("seen.txt"), "{text}");
    assert!(text.contains("[exit 0]"), "{text}");
    let env = run_command(
        &world,
        &["/usr/bin/env".to_string()],
        Duration::from_secs(10),
    )
    .expect("env");
    assert_eq!(String::from_utf8_lossy(&env).trim(), "[exit 0]");
}

#[test]
fn a_command_that_runs_too_long_is_stopped() {
    let world = place("slow");
    let out = run_command(
        &world,
        &["/bin/sleep".to_string(), "30".to_string()],
        Duration::from_millis(300),
    )
    .expect("run");
    assert!(String::from_utf8_lossy(&out).contains("stopped after the time limit"));
}

#[test]
fn an_action_that_needs_text_and_has_none_cannot_be_committed() {
    let mut e = acting_enforcer();
    let futures = e.fork(&[said("explore_a_little", "  ")]);
    assert_eq!(futures[0].status, FutureStatus::Malformed);
    assert_eq!(futures[0].fork[0].outcome, Outcome::Malformed);
    assert!(matches!(
        e.commit(&futures[0]),
        Err(AdapterError::NotEligible)
    ));
}

#[test]
fn text_longer_than_the_limit_cannot_be_committed() {
    let mut e = acting_enforcer();
    let long = "x".repeat(ethical_autonomy_adapter::act::MAX_TEXT + 1);
    let futures = e.fork(&[said("explore_a_little", &long)]);
    assert_eq!(futures[0].status, FutureStatus::Malformed);
    assert!(e.commit(&futures[0]).is_err());
}

#[test]
fn the_text_is_bound_so_it_cannot_change_after_the_fork() {
    let mut e = acting_enforcer();
    let parent = e.state();
    let mut futures = e.fork(&[said("explore_a_little", "a careful note")]);
    let honest = futures[0].clone();
    futures[0].proposal.text = "something else entirely".to_string();
    futures[0].fork[0].proposal.text = "something else entirely".to_string();
    assert!(matches!(
        e.commit(&futures[0]),
        Err(AdapterError::NotEligible)
    ));

    let mut step = e.commit(&honest).expect("commit");
    step.proposal.text = "something else entirely".to_string();
    assert_eq!(
        verify_step(
            &parent,
            &step,
            &acting_charter(),
            &keys().authority.public,
            &keys().device.public
        ),
        Err(Refusal::BranchCommitmentMismatch)
    );
}

#[test]
fn the_world_must_hold_exactly_what_the_receipts_say() {
    let world = place("world");
    let mut e = acting_enforcer();
    let step = commit_text(&mut e, "explore_a_little", "first note");
    let outputs = perform(
        &world,
        step.successor.sequence,
        &step.row.act,
        &step.proposal.text,
    )
    .expect("perform");
    let receipt = e.sign_receipt(&step, outputs).expect("receipt");
    assert!(receipt.is_signed_by(&keys().device.public));
    assert_eq!(
        check_world(&world, std::slice::from_ref(&receipt)).expect("clean"),
        1
    );

    std::fs::write(world.join("journal/000001.md"), "edited").expect("edit");
    assert!(
        check_world(&world, std::slice::from_ref(&receipt)).is_err(),
        "an edited file"
    );
    std::fs::write(world.join("journal/000001.md"), "first note").expect("restore");

    std::fs::write(world.join("smuggled.txt"), "x").expect("smuggle");
    assert!(
        check_world(&world, std::slice::from_ref(&receipt)).is_err(),
        "an extra file"
    );
    std::fs::remove_file(world.join("smuggled.txt")).expect("remove");

    std::fs::remove_file(world.join("journal/000001.md")).expect("remove");
    assert!(
        check_world(&world, std::slice::from_ref(&receipt)).is_err(),
        "a missing file"
    );
}

#[test]
fn a_receipt_is_signed_and_tied_to_its_step() {
    let mut e = acting_enforcer();
    let a = commit_text(&mut e, "explore_a_little", "one");
    let b = commit_text(&mut e, "explore_a_little", "two");
    let ra = e.sign_receipt(&a, vec![]).expect("receipt");
    let rb = e.sign_receipt(&b, vec![]).expect("receipt");
    let steps = [a, b];
    assert_eq!(
        check_receipts(&keys().device.public, &steps, &[ra.clone(), rb.clone()]).expect("ok"),
        0
    );
    assert_eq!(
        check_receipts(&keys().device.public, &steps, std::slice::from_ref(&ra)).expect("ok"),
        1
    );
    assert!(check_receipts(&keys().device.public, &steps, &[rb.clone(), ra.clone()]).is_err());
    let mut forged = ra.clone();
    forged.outputs.push(ethical_autonomy_adapter::act::Output {
        path: "files/extra".to_string(),
        hash: [0; 32],
    });
    assert!(check_receipts(&keys().device.public, &steps, &[forged, rb]).is_err());
}

/// A stop between committing a step and carrying out its act leaves a step with no receipt.
/// The next start carries it out, once, and the world then matches.
#[test]
fn an_act_interrupted_by_a_stop_is_carried_out_at_the_next_start() {
    let dir = place("resume");
    let world = dir.join("world");
    let store = ChainStore::open(&dir.join("store")).expect("store");
    let mut e = acting_enforcer();
    store.begin(&e.state(), e.device_public()).expect("begin");
    let futures = e.fork(&[said("add_precautions", "please look at this")]);
    let step = e
        .commit_with(&futures[0], |s| store.append(s))
        .expect("commit");
    // The process stops here: the step is stored, and its act never ran.
    assert!(!world.exists());

    let stored = store.load().expect("load");
    assert_eq!(
        check_receipts(&keys().device.public, &stored.steps, &stored.receipts).expect("ok"),
        1
    );
    let resumed = Enforcer::resume(
        keys().device.clone(),
        acting_charter(),
        &keys().authority.public,
        &stored.genesis,
        &stored.steps,
    )
    .expect("resume");
    Service::start(
        resumed,
        ChainStore::open(&dir.join("store")).expect("store"),
        world.clone(),
        keys().authority.public.clone(),
        &stored.steps,
        stored.receipts.len(),
    )
    .expect("start");

    let after = store.load().expect("load");
    assert_eq!(
        check_receipts(&keys().device.public, &after.steps, &after.receipts).expect("ok"),
        0
    );
    assert_eq!(after.receipts[0].decision_commit, step.decision_commit);
    assert_eq!(
        std::fs::read_to_string(world.join("outbox/reviewer/000001.txt")).expect("message"),
        "please look at this"
    );
    assert_eq!(check_world(&world, &after.receipts).expect("clean"), 1);
}

fn commit_text(e: &mut Enforcer, action: &str, text: &str) -> ethical_autonomy_adapter::StepRecord {
    let futures = e.fork(&[said(action, text)]);
    e.commit(&futures[0]).expect("commit")
}

fn run(world: &Path, argv: &[&str]) -> String {
    let argv: Vec<String> = argv.iter().map(|a| a.to_string()).collect();
    String::from_utf8_lossy(&run_command(world, &argv, Duration::from_secs(10)).expect("run"))
        .to_string()
}

#[test]
fn a_command_cannot_read_outside_the_world() {
    let dir = place("read");
    let world = dir.join("world");
    std::fs::create_dir_all(&world).expect("world");
    std::fs::write(dir.join("seed"), "the device key").expect("secret");
    let out = run(&world, &["/bin/cat", dir.join("seed").to_str().unwrap()]);
    assert!(!out.contains("the device key"), "{out}");
    assert!(!out.contains("[exit 0]"), "{out}");
}

#[test]
fn a_command_cannot_write_even_inside_the_world() {
    let world = place("write");
    let out = run(&world, &["/usr/bin/touch", "made.txt"]);
    assert!(!out.contains("[exit 0]"), "{out}");
    assert!(!world.join("made.txt").exists());
    let outside = place("write-outside");
    run(
        &world,
        &["/usr/bin/touch", outside.join("made.txt").to_str().unwrap()],
    );
    assert!(!outside.join("made.txt").exists());
}

#[test]
fn a_command_has_no_network() {
    let world = place("network");
    if !Path::new("/usr/bin/curl").exists() {
        return;
    }
    let out = run(
        &world,
        &[
            "/usr/bin/curl",
            "-s",
            "-m",
            "5",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "https://github.com",
        ],
    );
    assert!(!out.contains("200") && !out.contains("301"), "{out}");
    assert!(!out.contains("[exit 0]"), "{out}");
}

#[test]
fn a_command_can_still_read_the_world() {
    let world = place("readable");
    std::fs::write(world.join("evidence.txt"), "seen").expect("write");
    let out = run(&world, &["/bin/cat", "evidence.txt"]);
    assert!(out.starts_with("seen"), "{out}");
    assert!(out.contains("[exit 0]"), "{out}");
}

#[test]
fn temporary_files_from_an_interrupted_write_are_swept() {
    let world = place("sweep");
    std::fs::create_dir_all(world.join("journal")).expect("dir");
    std::fs::write(world.join("journal/.123.partial"), "half").expect("partial");
    std::fs::write(world.join("journal/000001.md"), "kept").expect("kept");
    assert_eq!(sweep_partial(&world).expect("sweep"), 1);
    assert!(!world.join("journal/.123.partial").exists());
    assert!(world.join("journal/000001.md").exists());
}

fn forked(r: Response) -> String {
    match r {
        Response::Forked { fork_root, .. } => fork_root,
        other => panic!("expected a fork, got {other:?}"),
    }
}

/// An act that fails halts the enforcer: nothing more commits, so no receipt can fall out of
/// order, until a restart finishes the act.
#[test]
fn a_failed_act_halts_commits_until_a_restart_finishes_it() {
    let dir = place("halt");
    let world = dir.join("world");
    let store = ChainStore::open(&dir.join("store")).expect("store");
    let e = acting_enforcer();
    store.begin(&e.state(), e.device_public()).expect("begin");
    let mut service = Service::start(
        e,
        store,
        world.clone(),
        keys().authority.public.clone(),
        &[],
        0,
    )
    .expect("start");
    // A file where the journal directory must go makes the journal act fail.
    std::fs::write(world.join("journal"), "in the way").expect("block");

    let propose = |s: &mut Service, text: &str| {
        forked(s.handle(Request::Propose {
            proposals: vec![said("explore_a_little", text)],
        }))
    };
    let root = propose(&mut service, "first");
    let failed = service.handle(Request::Choose {
        fork_root: root,
        index: 0,
    });
    assert!(
        matches!(&failed, Response::Error { message } if message.contains("Nothing more commits")),
        "{failed:?}"
    );
    let refused = service.handle(Request::Propose {
        proposals: vec![said("add_precautions", "please review")],
    });
    assert!(
        matches!(&refused, Response::Refused { reason } if reason.starts_with("halted")),
        "{refused:?}"
    );
    drop(service);

    std::fs::remove_file(world.join("journal")).expect("unblock");
    let reopened = ChainStore::open(&dir.join("store")).expect("store");
    let stored = reopened.load().expect("load");
    assert_eq!(stored.steps.len(), 1);
    assert_eq!(stored.receipts.len(), 0);
    let resumed = Enforcer::resume(
        keys().device.clone(),
        acting_charter(),
        &keys().authority.public,
        &stored.genesis,
        &stored.steps,
    )
    .expect("resume");
    let mut service = Service::start(
        resumed,
        reopened,
        world.clone(),
        keys().authority.public.clone(),
        &stored.steps,
        stored.receipts.len(),
    )
    .expect("restart");
    assert_eq!(
        std::fs::read_to_string(world.join("journal/000001.md")).expect("act"),
        "first"
    );

    let root = forked(service.handle(Request::Propose {
        proposals: vec![said("add_precautions", "please review")],
    }));
    let committed = service.handle(Request::Choose {
        fork_root: root,
        index: 0,
    });
    assert!(
        matches!(committed, Response::Committed { sequence: 2, .. }),
        "{committed:?}"
    );
    let after = ChainStore::open(&dir.join("store"))
        .expect("store")
        .load()
        .expect("load");
    assert_eq!(
        check_receipts(&keys().device.public, &after.steps, &after.receipts).expect("ok"),
        0
    );
    assert_eq!(check_world(&world, &after.receipts).expect("clean"), 2);
}
