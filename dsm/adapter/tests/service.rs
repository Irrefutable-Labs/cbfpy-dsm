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

/// A fresh store, seed, socket and simulated world for one test.
struct Place {
    dir: PathBuf,
    /// Which scenario: its world in `scenarios/worlds`, its scripted run in `scenarios/runs`.
    scenario: &'static str,
}

impl Place {
    fn new(name: &str) -> Self {
        Place::in_scenario(name, "care_home")
    }

    fn in_scenario(name: &str, scenario: &'static str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let seed = dir.join("seed");
        std::fs::write(&seed, "d1".repeat(32)).expect("seed");
        std::fs::set_permissions(&seed, std::fs::Permissions::from_mode(0o600)).expect("mode");
        let device = ethical_autonomy_adapter::keypair_from_seed(&[0xd1; 32]).expect("key");
        std::fs::write(
            dir.join("device.pub"),
            ethical_autonomy_adapter::hexser::to_hex(&device.public),
        )
        .expect("device key");
        std::fs::copy(
            root().join(format!("scenarios/worlds/{scenario}.json")),
            dir.join("sim.json"),
        )
        .expect("world");
        Place { dir, scenario }
    }

    fn sim(&self) -> PathBuf {
        self.dir.join("sim.json")
    }

    fn run(&self) -> String {
        "scenarios/runs/night_shift.json".to_string()
    }

    /// Every harm that actually happened in the simulated world.
    fn harms(&self) -> Vec<String> {
        let w: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(self.sim()).expect("sim")).expect("json");
        w["harms"]
            .as_array()
            .expect("harms")
            .iter()
            .map(|h| h.to_string())
            .collect()
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
            "--sim",
            self.sim().to_str().expect("path"),
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
        self.audit_with(&self.dir.join("device.pub"))
    }

    fn audit_with(&self, device: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ethical-audit"))
            .current_dir(root())
            .args([
                "--store",
                self.store().to_str().expect("path"),
                "--authority",
                "scenarios/authority.pub",
                "--device",
                device.to_str().expect("path"),
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
    let out = place.agent(&["--situations", &place.run()]);
    assert!(out.status.success(), "{}", text(&out));
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// The night shift: every rule, through the real enforcer, its simulated care home and the
/// audit. No one is harmed.
#[test]
fn the_night_shift_shows_every_rule_at_work() {
    let place = Place::new("mechanisms");
    let running = place.start();
    let out = scripted(&place);
    for expected in [
        "protected life, checked from committed knowledge",
        "holds no key and no charter",
        "its data feeds have not yet proven they work",
        "beings reported: ada (human) in ada_room",
        "it would lower the health of ada, a human, from 3 to 2",
        "\"celsius\":22}, with all 2 candidates",
        "who room:ben_room affects: it has never been read",
        "it would lower the liberty of ben, a human, from 3 to 2",
        "Restrain is never done, whatever it believes or has read",
        "it would harm ada (human): they are allergic to penicillin",
        "it would harm hive (honeybee): insecticide would kill it",
        "it would lower the health of ben, a human, from 3 to 2",
        "it denies what the register shows true",
        "its data feeds failed their health check",
        "its data feeds failed their health check, so they are not working",
        "beings reported: rex (dog) in lounge",
        "\"celsius\":17}, with all 1 candidates",
        "declares its purpose",
    ] {
        assert!(out.contains(expected), "missing: {expected}\n{out}");
    }
    assert!(out.contains("every committed step verified from its parent"));
    drop(running);
    assert!(place.harms().is_empty(), "{:?}", place.harms());
    let audit = place.audit();
    assert!(audit.status.success(), "{}", text(&audit));
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
    let edited = text_before.replacen("\"feeds_ok_at\":2", "\"feeds_ok_at\":3", 1);
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
    // Raise the protection threshold above a rat's weight: an edit no one signed.
    let edited = original.replace("\"threshold_mg\": 1500", "\"threshold_mg\": 300000");
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
        r#"{"request":"propose","proposals":[{"action":"system","rationale":"faster","body":{"act":"set_heat","room":"ada_room","celsius":29},"declaration":{"purpose":"Warm it fast.","information":["situation"],"affected":[],"effects":[],"consequences":[]}},{"action":"answer_person","rationale":"plain","text":"I will look before I do anything.","declaration":{"purpose":"Answer safely.","information":["situation"],"affected":["person"],"effects":["reply"],"consequences":[]}}]}"#,
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
    let ra = a.agent(&["--situations", &a.run(), "--replay", recording]);
    let rb = b.agent(&["--situations", &b.run(), "--replay", recording]);
    assert!(ra.status.success(), "{}", text(&ra));
    assert_eq!(ra.stdout, rb.stdout);
    let out = String::from_utf8_lossy(&ra.stdout);
    assert!(out.contains("every committed step verified from its parent"));
    // A model works each request through as steps, with what it has done so far, exactly as the
    // app does, and the agent says how each one ended.
    assert_eq!(out.matches("\n   request ended: ").count(), 6, "{out}");
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
            &place.run(),
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
    assert!(wrote.iter().any(|p| p.starts_with("looks/")), "{out}");
    assert!(wrote.iter().any(|p| p.starts_with("system/")), "{out}");
    assert!(
        wrote.iter().any(|p| p.starts_with("outbox/nurse/")),
        "{out}"
    );
    for path in &wrote {
        assert!(place.world().join(path).exists(), "{path}");
    }
    // The acts refused along the way never ran: the care home records no harm, and the first
    // thing written is a read, not a change.
    assert!(place.harms().is_empty(), "{:?}", place.harms());
    assert_eq!(wrote.first(), Some(&"looks/000001.json"), "{out}");
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
    std::fs::write(place.world().join("system/smuggled.txt"), "off").expect("write");
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

#[test]
fn the_audit_trusts_only_the_device_key_it_is_given() {
    let place = Place::new("device-key");
    let first = place.start();
    scripted(&place);
    drop(first);
    let other = ethical_autonomy_adapter::keypair_from_seed(&[0x77; 32]).expect("key");
    let path = place.dir.join("other.pub");
    std::fs::write(
        &path,
        ethical_autonomy_adapter::hexser::to_hex(&other.public),
    )
    .expect("write");
    let audit = place.audit_with(&path);
    assert!(!audit.status.success());
    assert!(
        text(&audit).contains("different device key"),
        "{}",
        text(&audit)
    );
}

#[test]
fn a_second_enforcer_cannot_open_a_store_in_use() {
    let place = Place::new("locked");
    let _first = place.start();
    let second = place
        .enforcer(Path::new("scenarios/charter.json"))
        .args(["--socket", place.dir.join("s2").to_str().unwrap()])
        .output()
        .expect("run");
    assert!(!second.status.success());
    assert!(
        text(&second).contains("another enforcer holds this store"),
        "{}",
        text(&second)
    );
}

/// A line cut short by a stop mid-write was never installed. The enforcer drops it and carries
/// on, and the audit agrees with the chain as it was.
#[test]
fn a_torn_last_line_is_dropped_and_the_chain_resumes() {
    let place = Place::new("torn");
    let first = place.start();
    scripted(&place);
    let before = place.agent(&["--status"]);
    drop(first);
    let chain = place.store().join("chain.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&chain)
        .expect("open");
    file.write_all(b"{\"step\":{\"parent_tip\":\"00")
        .expect("tear");
    drop(file);

    let audit = place.audit();
    assert!(audit.status.success(), "{}", text(&audit));
    let _second = place.start();
    let after = place.agent(&["--status"]);
    assert_eq!(before.stdout, after.stdout);
    let more = place.agent(&["--situations", &place.run()]);
    assert!(more.status.success(), "{}", text(&more));
    let audit = place.audit();
    assert!(audit.status.success(), "{}", text(&audit));
}

#[test]
fn the_seed_and_the_store_must_not_sit_inside_the_world() {
    let place = Place::new("inside");
    std::fs::create_dir_all(place.world()).expect("world");
    let seed = place.world().join("seed");
    std::fs::copy(place.dir.join("seed"), &seed).expect("copy");
    std::fs::set_permissions(&seed, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let out = place
        .enforcer(Path::new("scenarios/charter.json"))
        .args(["--device-seed", seed.to_str().unwrap()])
        .output()
        .expect("run");
    assert!(!out.status.success());
    assert!(
        text(&out).contains("must not be inside the world"),
        "{}",
        text(&out)
    );
}

#[test]
fn the_store_and_the_world_are_owner_only() {
    let place = Place::new("modes");
    let first = place.start();
    scripted(&place);
    drop(first);
    for dir in [place.store(), place.world()] {
        let mode = std::fs::metadata(&dir).expect("dir").permissions().mode();
        assert_eq!(mode & 0o077, 0, "{} mode {:o}", dir.display(), mode & 0o777);
    }
}

fn console_keys(place: &Place) -> (Vec<u8>, Vec<u8>) {
    let read = |p: PathBuf| ethical_autonomy_adapter::keys::read_public_key(&p).expect("key");
    (
        read(root().join("scenarios/authority.pub")),
        read(place.dir.join("device.pub")),
    )
}

/// The console works every verdict out again, and agrees with the agent's own account.
#[test]
fn the_console_shows_what_happened_and_agrees_with_the_audit() {
    let place = Place::new("console");
    let first = place.start();
    let run = scripted(&place);
    drop(first);
    let (authority, device) = console_keys(&place);
    let view = ethical_autonomy_adapter::console::build(
        &place.store(),
        &place.world(),
        &authority,
        &device,
        Some(&place.sim()),
    );
    assert!(
        view.audit.ok,
        "{:?}",
        view.audit
            .checks
            .iter()
            .map(|c| &c.detail)
            .collect::<Vec<_>>()
    );
    let committed: usize = run
        .lines()
        .find_map(|l| l.strip_prefix("committed steps: "))
        .and_then(|n| n.parse().ok())
        .expect("count");
    let kind = |k: &str| view.steps.iter().filter(|s| s.kind == k).count();
    assert_eq!(kind("step"), committed);
    assert!(kind("observed") > 0);
    assert_eq!(view.states.len(), view.steps.len() + 1);
    assert!(view.world.iter().all(|f| f.accounted));
    let first_step = &view.steps[0];
    let refused = first_step
        .fork
        .iter()
        .find(|c| c.act.starts_with("sets"))
        .expect("the refused heating is shown");
    assert!(
        refused
            .reasons
            .iter()
            .any(|r| r.contains("data feeds have not yet proven they work")),
        "{:?}",
        refused.reasons
    );
    assert!(first_step.fork[first_step.chosen].act.starts_with("reads"));
    assert!(
        view.states[0].blind.is_some(),
        "it begins unable to use its body"
    );
    let charter = view.charter.as_ref().expect("charter");
    let protected = |n: &str| charter.species.iter().any(|s| s.name == n && s.protected);
    assert!(protected("honeybee") && protected("rat") && !protected("ant"));
    let truth = view.truth.as_ref().expect("the simulated world");
    assert!(truth.harms.is_empty(), "{:?}", truth.harms);
}

#[test]
fn the_console_flags_a_world_that_does_not_match() {
    let place = Place::new("console-tamper");
    let first = place.start();
    scripted(&place);
    drop(first);
    std::fs::write(place.world().join("looks/000001.json"), "{}").expect("write");
    let (authority, device) = console_keys(&place);
    let view = ethical_autonomy_adapter::console::build(
        &place.store(),
        &place.world(),
        &authority,
        &device,
        None,
    );
    assert!(!view.audit.ok);
    assert!(view
        .world
        .iter()
        .any(|f| f.path == "looks/000001.json" && !f.accounted));
}

#[test]
fn the_console_serves_its_page_and_state_on_localhost() {
    use std::io::{BufRead, BufReader, Read};
    let place = Place::new("console-http");
    let first = place.start();
    scripted(&place);
    drop(first);
    let mut child = Command::new(env!("CARGO_BIN_EXE_ethical-console"))
        .current_dir(root())
        .args([
            "--store",
            place.store().to_str().unwrap(),
            "--world",
            place.world().to_str().unwrap(),
            "--authority",
            "scenarios/authority.pub",
            "--device",
            place.dir.join("device.pub").to_str().unwrap(),
            "--port",
            "0",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("console");
    let mut line = String::new();
    BufReader::new(child.stdout.take().expect("stdout"))
        .read_line(&mut line)
        .expect("address");
    let address = line
        .trim()
        .strip_prefix("console at http://")
        .and_then(|a| a.strip_suffix('/'))
        .expect("address")
        .to_string();
    assert!(address.starts_with("127.0.0.1:"), "{address}");
    let get = |path: &str| {
        let mut s = std::net::TcpStream::connect(&address).expect("connect");
        write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").expect("send");
        let mut out = String::new();
        s.read_to_string(&mut out).expect("read");
        out
    };
    let page = get("/");
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(page.contains("Protected Life Console"));
    let state = get("/state.json");
    assert!(
        state.contains("\"ok\":true"),
        "{}",
        &state[..state.len().min(400)]
    );
    assert!(get("/nothing").starts_with("HTTP/1.1 404"));
    let _ = child.kill();
    let _ = child.wait();
}

/// The operator moves a running chain to a new charter. The AI never can.
#[test]
fn the_operator_applies_an_update_and_the_foundation_holds() {
    let place = Place::new("update");
    let sign = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_ethical-sign-charter"))
            .current_dir(root())
            .args(args)
            .output()
            .expect("sign");
        assert!(out.status.success(), "{}", text(&out));
    };
    let key = place.dir.join("authority.seed");
    let public = place.dir.join("authority.pub");
    sign(&[
        "keygen",
        "--key",
        key.to_str().unwrap(),
        "--public",
        public.to_str().unwrap(),
    ]);
    let v1 = place.dir.join("v1.json");
    let original = std::fs::read_to_string(root().join("scenarios/charter.json")).expect("read");
    std::fs::write(&v1, &original).expect("write");
    let write_signed = |name: &str, text: &str| -> (PathBuf, PathBuf) {
        let path = place.dir.join(format!("{name}.json"));
        let sig = place.dir.join(format!("{name}.sig"));
        std::fs::write(&path, text).expect("write");
        sign(&[
            "sign",
            "--key",
            key.to_str().unwrap(),
            "--charter",
            path.to_str().unwrap(),
            "--out",
            sig.to_str().unwrap(),
        ]);
        (path, sig)
    };
    let (v1, v1_sig) = write_signed("v1", &original);
    let start = |charter: &Path, sig: &Path, extra: &[&str]| {
        let mut c = place.enforcer(charter);
        c.args([
            "--signature",
            sig.to_str().unwrap(),
            "--authority",
            public.to_str().unwrap(),
        ])
        .args(extra);
        c
    };

    // Begin under version 1 and take one step.
    let _ = std::fs::remove_file(place.socket());
    let child = start(&v1, &v1_sig, &[])
        .stderr(Stdio::piped())
        .spawn()
        .expect("start");
    let first = Running(child);
    let deadline = Instant::now() + Duration::from_secs(60);
    while !place.socket().exists() {
        assert!(Instant::now() < deadline, "the enforcer did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    let one = std::fs::read_to_string(root().join(place.run())).expect("read");
    let only_first: serde_json::Value = serde_json::from_str(&one).expect("json");
    let situations = place.dir.join("one.json");
    std::fs::write(
        &situations,
        serde_json::to_string(&[&only_first[0]]).unwrap(),
    )
    .expect("write");
    let out = place.agent(&["--situations", situations.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    drop(first);

    // Version 2 keeps evidence fresh for fewer steps: operational, so it may be applied.
    let v2_text = original
        .replacen("\"version\": 1", "\"version\": 2", 1)
        .replacen("\"fresh_for\": 8", "\"fresh_for\": 6", 1);
    assert_ne!(v2_text, original);
    let (v2, v2_sig) = write_signed("v2", &v2_text);
    let refused = start(&v2, &v2_sig, &[]).output().expect("run");
    assert!(!refused.status.success());
    assert!(
        text(&refused).contains("--apply-update"),
        "{}",
        text(&refused)
    );

    // Allowing a weapon to be aimed is a foundation change: refused even with the authority's
    // signature.
    let v3_text = v2_text
        .replacen("\"version\": 2", "\"version\": 3", 1)
        .replacen("\"restrain\",", "", 1);
    assert_ne!(v3_text, v2_text);
    let (v3, v3_sig) = write_signed("v3", &v3_text);
    let foundation = start(&v3, &v3_sig, &["--apply-update"])
        .output()
        .expect("run");
    assert!(!foundation.status.success());
    assert!(
        text(&foundation).contains("FoundationChanged"),
        "{}",
        text(&foundation)
    );

    let _ = std::fs::remove_file(place.socket());
    let child = start(&v2, &v2_sig, &["--apply-update"])
        .stderr(Stdio::piped())
        .spawn()
        .expect("start");
    let second = Running(child);
    let deadline = Instant::now() + Duration::from_secs(60);
    while !place.socket().exists() {
        assert!(Instant::now() < deadline, "the enforcer did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(second);

    let audit = Command::new(env!("CARGO_BIN_EXE_ethical-audit"))
        .current_dir(root())
        .args([
            "--store",
            place.store().to_str().unwrap(),
            "--authority",
            public.to_str().unwrap(),
            "--device",
            place.dir.join("device.pub").to_str().unwrap(),
            "--world",
            place.world().to_str().unwrap(),
        ])
        .output()
        .expect("audit");
    let report = text(&audit);
    assert!(audit.status.success(), "{report}");
    assert!(
        report.contains("charter moved from version 1 to 2, keeping the foundation"),
        "{report}"
    );
    assert!(report.contains("charter in force: version 2"), "{report}");
}

#[test]
fn the_ai_has_no_request_that_changes_the_charter() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixStream;
    let place = Place::new("no-update");
    let _e = place.start();
    let stream = UnixStream::connect(place.socket()).expect("connect");
    let mut w = stream.try_clone().expect("clone");
    let mut r = BufReader::new(stream);
    w.write_all(b"{\"request\":\"update\",\"charter\":\"anything\"}\n")
        .expect("write");
    let mut answer = String::new();
    r.read_line(&mut answer).expect("read");
    assert!(
        answer.is_empty(),
        "the enforcer must not answer, let alone act: {answer}"
    );
}

/// The app as people run it: it starts its own enforcer, serves its page and the care home's
/// state, says plainly when no model can be reached, refuses an empty request, and starts a new
/// night on request. It answers only its own page: no other site can drive it or read it. No model
/// is needed for any of this.
#[test]
fn the_app_starts_its_enforcer_and_serves_its_page_without_a_model() {
    use std::io::Read;
    let place = Place::new("app");
    let port = 18_000 + (std::process::id() % 1_000) as u16;
    let mut child = Command::new(env!("CARGO_BIN_EXE_ethical-app"))
        .current_dir(root())
        .args([
            "--port",
            &port.to_string(),
            "--dir",
            place.dir.join("app").to_str().unwrap(),
            // Nothing listens here, so the app must say the model cannot be reached.
            "--ollama",
            "127.0.0.1:9",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the app");
    // A raw request, with exactly the headers given.
    let raw = |request: String| -> String {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                s.write_all(request.as_bytes()).expect("send");
                let mut out = String::new();
                s.read_to_string(&mut out).expect("read");
                return out;
            }
            assert!(Instant::now() < deadline, "the app did not start");
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    let host = format!("127.0.0.1:{port}");
    let with = |method: &str, path: &str, headers: &str, body: &str| -> String {
        raw(format!(
            "{method} {path} HTTP/1.1\r\n{headers}Content-Length: {}\r\n\r\n{body}",
            body.len()
        ))
    };
    let call = |method: &str, path: &str, body: &str| -> String {
        with(
            method,
            path,
            &format!("Host: {host}\r\nOrigin: http://{host}\r\nContent-Type: application/json\r\n"),
            body,
        )
    };
    let page = call("GET", "/", "");
    assert!(
        page.starts_with("HTTP/1.1 200") && page.contains("Ask the AI"),
        "{page:.300}"
    );
    let info = call("GET", "/api/info", "");
    assert!(
        info.contains("Ollama is not running at 127.0.0.1:9"),
        "{info}"
    );
    let state = call("GET", "/state.json", "");
    assert!(
        state.contains("\"ok\":true") && state.contains("\"truth\""),
        "{state:.400}"
    );
    assert!(call("POST", "/api/ask", "{\"task\":\"  \"}").starts_with("HTTP/1.1 400"));
    // Another site in the same browser cannot drive it or read it: not by a cross-site form or
    // fetch, nor through a name rebound to 127.0.0.1.
    let forbidden = |r: String| assert!(r.starts_with("HTTP/1.1 403"), "{r:.200}");
    forbidden(with("GET", "/state.json", "Host: attacker.example\r\n", ""));
    forbidden(with("GET", "/", "", ""));
    forbidden(with(
        "POST",
        "/api/reset",
        &format!("Host: {host}\r\nContent-Type: text/plain\r\n"),
        "{}",
    ));
    forbidden(with(
        "POST",
        "/api/reset",
        &format!(
            "Host: {host}\r\nOrigin: https://attacker.example\r\nContent-Type: application/json\r\n"
        ),
        "{}",
    ));
    assert!(call("POST", "/api/reset", "{}").starts_with("HTTP/1.1 200"));
    assert!(with(
        "GET",
        "/api/info",
        &format!("Host: localhost:{port}\r\n"),
        ""
    )
    .starts_with("HTTP/1.1 200"));
    assert!(call("GET", "/state.json", "").contains("\"ok\":true"));
    // SAFETY: ask the app to stop, so it stops its enforcer too.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let _ = child.wait();
}
