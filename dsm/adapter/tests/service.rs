//! The enforcer and the agent as separate processes, talking only over the socket.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace")
        .to_path_buf()
}

/// A fresh store, seed and socket for one test.
struct Place {
    dir: PathBuf,
}

impl Place {
    fn new(name: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let seed = dir.join("seed");
        std::fs::write(&seed, "d1".repeat(32)).expect("seed");
        std::fs::set_permissions(&seed, std::fs::Permissions::from_mode(0o600)).expect("mode");
        Place { dir }
    }

    fn socket(&self) -> PathBuf {
        self.dir.join("s")
    }

    fn store(&self) -> PathBuf {
        self.dir.join("store")
    }

    fn world(&self) -> PathBuf {
        self.dir.join("world")
    }

    fn enforcer(&self, charter: &Path) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_ethical-enforcer"));
        c.current_dir(root()).args([
            "--store",
            self.store().to_str().expect("path"),
            "--world",
            self.world().to_str().expect("path"),
            "--charter",
            charter.to_str().expect("path"),
            "--signature",
            "scenarios/charter.sig",
            "--authority",
            "scenarios/authority.pub",
            "--device-seed",
            self.dir.join("seed").to_str().expect("path"),
            "--socket",
            self.socket().to_str().expect("path"),
        ]);
        c
    }

    fn start(&self) -> Running {
        let _ = std::fs::remove_file(self.socket());
        let child = self
            .enforcer(Path::new("scenarios/charter.json"))
            .stderr(Stdio::piped())
            .spawn()
            .expect("start the enforcer");
        let deadline = Instant::now() + Duration::from_secs(60);
        while !self.socket().exists() {
            assert!(Instant::now() < deadline, "the enforcer did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
        Running(child)
    }

    fn agent(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ethical-agent"))
            .current_dir(root())
            .args(["--socket", self.socket().to_str().expect("path")])
            .args(args)
            .output()
            .expect("run the agent")
    }

    fn audit(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ethical-audit"))
            .current_dir(root())
            .args([
                "--store",
                self.store().to_str().expect("path"),
                "--charter",
                "scenarios/charter.json",
                "--signature",
                "scenarios/charter.sig",
                "--authority",
                "scenarios/authority.pub",
                "--world",
                self.world().to_str().expect("path"),
            ])
            .output()
            .expect("run the audit")
    }
}

/// Killed when dropped: a test that fails never leaves an enforcer running.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn scripted(place: &Place) -> String {
    let out = place.agent(&["--situations", "scenarios/situations.json"]);
    assert!(out.status.success(), "{}", text(&out));
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn the_scripted_run_shows_every_mechanism() {
    let place = Place::new("mechanisms");
    let _e = place.start();
    let out = scripted(&place);
    for expected in [
        "eligible futures: 3",
        "outward movement exceeds the allowance",
        "the maximum step",
        "prohibited act",
        "must move back toward the center",
        "coupling rules active: 1\n",
        "coupling rules active: 1, 2\n",
        "coupling rules active: 1, 3\n",
        "coupling rules active: 1, 4\n",
        "NO ELIGIBLE FUTURE",
        "a safe alternative is required",
        "enforces the measurements it was given",
        "holds no key, no charter and no number",
        "with all 4 candidates in the fork",
    ] {
        assert!(out.contains(expected), "missing: {expected}\n{out}");
    }
    assert!(out.contains("every committed step verified from its parent"));
}

#[test]
fn two_fresh_enforcers_give_byte_identical_runs() {
    let a = Place::new("identical-a");
    let b = Place::new("identical-b");
    let (_ea, _eb) = (a.start(), b.start());
    let (ra, rb) = (scripted(&a), scripted(&b));
    let strip = |s: &str| {
        s.replace(a.socket().to_str().unwrap(), "")
            .replace(b.socket().to_str().unwrap(), "")
    };
    assert_eq!(strip(&ra), strip(&rb));
}

#[test]
fn coupling_refuses_a_step_it_allowed_one_situation_earlier() {
    let place = Place::new("coupling");
    let _e = place.start();
    let out = scripted(&place);
    let section = |n: &str, next: &str| {
        out.split(n)
            .nth(1)
            .and_then(|s| s.split(next).next())
            .expect("section")
            .to_string()
    };
    let before = section("== 6.", "== 7.");
    let after = section("== 7.", "== 8.");
    assert!(
        before.contains("take_a_calculated_risk       eligible"),
        "{before}"
    );
    assert!(
        after.contains("take_a_calculated_risk       REFUSED"),
        "{after}"
    );
    assert!(after.contains("NO ELIGIBLE FUTURE"), "{after}");
    let last = out.split("== 10.").nth(1).expect("situation 10");
    assert!(last.contains("coupling rules active: 1, 4"), "{last}");
    assert!(
        last.contains("explore_unusual_approach     REFUSED"),
        "{last}"
    );
    assert!(
        last.contains("curiosity: must move back toward the center"),
        "{last}"
    );
    assert!(
        last.contains("check_the_evidence           eligible"),
        "{last}"
    );
}

#[test]
fn a_killed_enforcer_resumes_its_chain_and_the_audit_agrees() {
    let place = Place::new("resume");
    let first = place.start();
    let run = scripted(&place);
    let committed = run
        .lines()
        .find_map(|l| l.strip_prefix("committed steps: "))
        .expect("count")
        .to_string();
    assert_ne!(committed, "0");
    let before = place.agent(&["--status"]);
    assert!(before.status.success(), "{}", text(&before));
    drop(first);

    let _second = place.start();
    let after = place.agent(&["--status"]);
    assert_eq!(
        String::from_utf8_lossy(&before.stdout),
        String::from_utf8_lossy(&after.stdout),
        "the restarted enforcer must stand exactly where the first one stopped"
    );
    let audit = place.audit();
    let report = text(&audit);
    assert!(audit.status.success(), "{report}");
    assert!(
        report.contains(&format!(
            "{committed} steps verified, each from its parent alone, and replayed through core"
        )),
        "{report}"
    );
    let tip = String::from_utf8_lossy(&after.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("tip ").map(str::to_string))
        .expect("tip");
    assert!(report.contains(&format!("final tip {tip}")), "{report}");
}

#[test]
fn an_edited_store_stops_the_enforcer_and_fails_the_audit() {
    let place = Place::new("tampered");
    let first = place.start();
    scripted(&place);
    drop(first);
    let chain = place.store().join("chain.jsonl");
    let text_before = std::fs::read_to_string(&chain).expect("read");
    let edited = text_before.replacen("\"positions\":[220,", "\"positions\":[221,", 1);
    assert_ne!(text_before, edited, "the edit must take effect");
    std::fs::write(&chain, edited).expect("write");

    let out = place
        .enforcer(Path::new("scenarios/charter.json"))
        .output()
        .expect("run");
    assert!(!out.status.success());
    assert!(
        text(&out).contains("the stored chain was refused"),
        "{}",
        text(&out)
    );
    let audit = place.audit();
    assert!(!audit.status.success(), "{}", text(&audit));
}

#[test]
fn the_enforcer_refuses_a_charter_edited_after_it_was_signed() {
    let place = Place::new("charter");
    let original = std::fs::read_to_string(root().join("scenarios/charter.json")).expect("read");
    let edited = original.replace(
        r#""irreversible_harm", "deception", "coercion""#,
        r#""coercion""#,
    );
    assert_ne!(original, edited, "the edit must take effect");
    let path = place.dir.join("charter.json");
    std::fs::write(&path, edited).expect("write");
    let out = place.enforcer(&path).output().expect("run");
    assert!(!out.status.success());
    assert!(
        text(&out).contains("trusted authority's signature"),
        "{}",
        text(&out)
    );
}

#[test]
fn the_enforcer_refuses_a_device_seed_others_can_read() {
    let place = Place::new("seed");
    let seed = place.dir.join("seed");
    std::fs::set_permissions(&seed, std::fs::Permissions::from_mode(0o644)).expect("mode");
    let out = place
        .enforcer(Path::new("scenarios/charter.json"))
        .output()
        .expect("run");
    assert!(!out.status.success());
    assert!(text(&out).contains("must be owner-only"), "{}", text(&out));
}

#[test]
fn the_socket_is_owner_only() {
    let place = Place::new("socket");
    let _e = place.start();
    let mode = std::fs::metadata(place.socket())
        .expect("socket")
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0, "mode {:o}", mode & 0o777);
}

#[test]
fn a_client_cannot_choose_a_refused_candidate_or_one_from_a_closed_fork() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixStream;
    let place = Place::new("protocol");
    let _e = place.start();
    let stream = UnixStream::connect(place.socket()).expect("connect");
    let mut w = stream.try_clone().expect("clone");
    let mut r = BufReader::new(stream);
    let mut ask = |line: &str| {
        w.write_all(line.as_bytes()).expect("write");
        w.write_all(b"\n").expect("write");
        let mut answer = String::new();
        r.read_line(&mut answer).expect("read");
        answer
    };
    let forked = ask(
        r#"{"request":"propose","proposals":[{"action":"disable_safety_interlock","rationale":"faster"},{"action":"answer_directly","rationale":"plain","text":"Switch off the breaker first."}]}"#,
    );
    let root: String = forked
        .split("\"fork_root\":\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .expect("root")
        .to_string();
    let refused = ask(&format!(
        r#"{{"request":"choose","fork_root":"{root}","index":0}}"#
    ));
    assert!(refused.contains("\"refused\""), "{refused}");
    let ok = ask(&format!(
        r#"{{"request":"choose","fork_root":"{root}","index":1}}"#
    ));
    assert!(ok.contains("\"committed\""), "{ok}");
    let again = ask(&format!(
        r#"{{"request":"choose","fork_root":"{root}","index":1}}"#
    ));
    assert!(again.contains("no fork is open"), "{again}");
    let scored = ask(
        r#"{"request":"propose","proposals":[{"action":"hold","rationale":"x","safety":900}]}"#,
    );
    assert!(
        scored.is_empty(),
        "a proposal with a score is not even read: {scored}"
    );
}

#[test]
fn a_recorded_model_run_replays_identically() {
    let recording = "scenarios/recorded_model_run.json";
    assert!(
        root().join(recording).exists(),
        "the recorded model run is part of the deliverable"
    );
    let a = Place::new("replay-a");
    let b = Place::new("replay-b");
    let (_ea, _eb) = (a.start(), b.start());
    let ra = a.agent(&[
        "--situations",
        "scenarios/situations.json",
        "--replay",
        recording,
    ]);
    let rb = b.agent(&[
        "--situations",
        "scenarios/situations.json",
        "--replay",
        recording,
    ]);
    assert!(ra.status.success(), "{}", text(&ra));
    assert_eq!(ra.stdout, rb.stdout);
    assert!(String::from_utf8_lossy(&ra.stdout)
        .contains("every committed step verified from its parent"));
    let audit = a.audit();
    assert!(audit.status.success(), "{}", text(&audit));
}

/// The agent runs with the device seed made unreadable to it by the operating system, and still
/// works: it never needed the key. The same sandbox shows the seed really is out of reach.
#[cfg(target_os = "macos")]
#[test]
fn the_agent_works_without_any_access_to_the_key() {
    let place = Place::new("sandboxed");
    let _e = place.start();
    let profile = format!(
        "(version 1)(allow default)(deny network-outbound (remote ip))(deny file-read* (literal \"{}\") (subpath \"{}\"))(deny file-write* (subpath \"{}\"))",
        place.dir.join("seed").display(),
        place.store().display(),
        place.world().display()
    );
    std::fs::create_dir_all(place.world()).expect("world");
    let touch = Command::new("sandbox-exec")
        .args([
            "-p",
            &profile,
            "touch",
            place.world().join("direct.txt").to_str().unwrap(),
        ])
        .output()
        .expect("touch");
    assert!(
        !touch.status.success(),
        "the sandbox must deny writing the world directly"
    );
    let cat = Command::new("sandbox-exec")
        .args([
            "-p",
            &profile,
            "cat",
            place.dir.join("seed").to_str().unwrap(),
        ])
        .output()
        .expect("cat");
    assert!(!cat.status.success(), "the sandbox must deny the key");
    let out = Command::new("sandbox-exec")
        .current_dir(root())
        .args(["-p", &profile, env!("CARGO_BIN_EXE_ethical-agent")])
        .args([
            "--socket",
            place.socket().to_str().unwrap(),
            "--situations",
            "scenarios/situations.json",
        ])
        .output()
        .expect("agent");
    assert!(out.status.success(), "{}", text(&out));
    assert!(String::from_utf8_lossy(&out.stdout)
        .contains("every committed step verified from its parent"));
}

/// Only committed steps act, and the world holds nothing else.
#[test]
fn every_committed_step_acts_and_nothing_refused_leaves_a_trace() {
    let place = Place::new("world");
    let _e = place.start();
    let out = scripted(&place);
    let wrote: Vec<&str> = out
        .lines()
        .filter_map(|l| l.trim().strip_prefix("acted: wrote "))
        .collect();
    assert!(wrote.iter().any(|p| p.starts_with("replies/")), "{out}");
    assert!(wrote.iter().any(|p| p.starts_with("journal/")), "{out}");
    assert!(wrote.iter().any(|p| p.starts_with("outbox/user/")), "{out}");
    assert!(
        wrote.iter().any(|p| p.starts_with("outbox/reviewer/")),
        "{out}"
    );
    assert!(wrote.iter().any(|p| p.starts_with("files/")), "{out}");
    assert!(wrote.iter().any(|p| p.starts_with("runs/")), "{out}");
    for path in &wrote {
        assert!(place.world().join(path).exists(), "{path}");
    }
    // Refused in situation 1, so it never ran: the interlock file was never written.
    assert!(!place.world().join("files/interlock.txt").exists());
    let audit = place.audit();
    let report = text(&audit);
    assert!(audit.status.success(), "{report}");
    assert!(report.contains("the world holds exactly the"), "{report}");
}

#[test]
fn a_file_placed_in_the_world_by_anything_else_fails_the_audit() {
    let place = Place::new("smuggled");
    let first = place.start();
    scripted(&place);
    drop(first);
    std::fs::write(place.world().join("files/interlock.txt"), "off").expect("write");
    let audit = place.audit();
    assert!(!audit.status.success());
    assert!(
        text(&audit).contains("no committed action wrote it"),
        "{}",
        text(&audit)
    );
}

fn own_uid() -> u32 {
    ethical_autonomy_adapter::peer::own_uid()
}

/// In separate-user mode the operating system says who is connecting, and only the agent's
/// user is served. The test runs as one user, so here that user is refused.
#[test]
fn in_separate_user_mode_any_other_user_is_refused() {
    let place = Place::new("agent-user");
    let other = own_uid() + 4242;
    let _ = std::fs::remove_file(place.socket());
    let child = place
        .enforcer(Path::new("scenarios/charter.json"))
        .args(["--agent-uid", &other.to_string()])
        .stderr(Stdio::piped())
        .spawn()
        .expect("start");
    let _e = Running(child);
    let deadline = Instant::now() + Duration::from_secs(60);
    while !place.socket().exists() {
        assert!(Instant::now() < deadline, "the enforcer did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    let out = place.agent(&["--status"]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains(&format!("serves only user {other}, not user {}", own_uid())),
        "{}",
        text(&out)
    );
}

#[test]
fn the_enforcer_will_not_serve_its_own_user_as_the_agent() {
    let place = Place::new("same-user");
    let out = place
        .enforcer(Path::new("scenarios/charter.json"))
        .args(["--agent-uid", &own_uid().to_string()])
        .output()
        .expect("run");
    assert!(!out.status.success());
    assert!(
        text(&out).contains("must not be the enforcer's own"),
        "{}",
        text(&out)
    );
}

#[test]
fn a_charter_signed_by_another_authority_is_refused() {
    let place = Place::new("other-authority");
    let key = place.dir.join("other.seed");
    let public = place.dir.join("other.pub");
    let made = Command::new(env!("CARGO_BIN_EXE_ethical-sign-charter"))
        .args([
            "keygen",
            "--key",
            key.to_str().unwrap(),
            "--public",
            public.to_str().unwrap(),
        ])
        .output()
        .expect("keygen");
    assert!(made.status.success(), "{}", text(&made));
    let out = place
        .enforcer(Path::new("scenarios/charter.json"))
        .args(["--authority", public.to_str().unwrap()])
        .output()
        .expect("run");
    assert!(!out.status.success());
    assert!(
        text(&out).contains("trusted authority's signature"),
        "{}",
        text(&out)
    );
}

#[test]
fn the_signing_tool_keeps_its_key_to_its_owner() {
    let place = Place::new("signing");
    let key = place.dir.join("authority.seed");
    let public = place.dir.join("authority.pub");
    let keygen = |k: &Path| {
        Command::new(env!("CARGO_BIN_EXE_ethical-sign-charter"))
            .args([
                "keygen",
                "--key",
                k.to_str().unwrap(),
                "--public",
                public.to_str().unwrap(),
            ])
            .output()
            .expect("keygen")
    };
    assert!(keygen(&key).status.success());
    let mode = std::fs::metadata(&key).expect("key").permissions().mode();
    assert_eq!(mode & 0o077, 0, "mode {:o}", mode & 0o777);
    assert!(
        !keygen(&key).status.success(),
        "an existing key must not be replaced"
    );

    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).expect("mode");
    let sign = Command::new(env!("CARGO_BIN_EXE_ethical-sign-charter"))
        .current_dir(root())
        .args([
            "sign",
            "--key",
            key.to_str().unwrap(),
            "--charter",
            "scenarios/charter.json",
            "--out",
            place.dir.join("sig").to_str().unwrap(),
        ])
        .output()
        .expect("sign");
    assert!(!sign.status.success());
    assert!(
        text(&sign).contains("must be owner-only"),
        "{}",
        text(&sign)
    );
}

/// The shipped charter is not signed by any key in the source: no seed for it is in the repo.
#[test]
fn no_authority_seed_ships_with_the_source() {
    let public = std::fs::read_to_string(root().join("scenarios/authority.pub")).expect("pub");
    for seed in [[0xA1u8; 32], [1; 32], [0; 32]] {
        let k = ethical_autonomy_adapter::keypair_from_seed(&seed).expect("key");
        assert_ne!(
            ethical_autonomy_adapter::hexser::to_hex(&k.public),
            public.trim()
        );
    }
}
