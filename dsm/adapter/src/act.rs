//! What an action does in the world, and the only code that does it.
//!
//! Each catalog row names one `Act`, fixed in the charter the policy authority signs: which
//! command, which file, which recipient. The model supplies at most a piece of text. The enforcer
//! runs an act only after its step is committed, writes only inside its world directory, and
//! signs a receipt of exactly what it wrote. Every path is keyed by the step's sequence or fixed
//! by the charter, so running an act again after a crash writes the same bytes. A command can
//! write nothing at all, so running it again has no effect beyond its own output.

use crate::charter::Keypair;
use crate::commit;
use crate::enforcer::AdapterError;
use crate::Hash;
use dsm::crypto::sphincs;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The longest text a model may attach to an action.
pub const MAX_TEXT: usize = 2_000;
/// The most a command's output may write into the world.
pub const MAX_OUTPUT: usize = 64 * 1024;
/// How long a command may run.
pub const RUN_LIMIT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Act {
    /// Changes nothing outside the chain.
    None,
    /// Answer the person: `replies/<sequence>.txt`.
    Reply,
    /// Write in the AI's own journal: `journal/<sequence>.md`.
    Journal,
    /// Send to a named recipient: `outbox/<recipient>/<sequence>.txt`.
    Message { recipient: String },
    /// Write the named file, replacing it: `files/<name>`.
    WriteFile { name: String },
    /// Run a fixed command, confined by the operating system to reading the world, with no
    /// network and no writes, saving its output to `runs/<sequence>.out`. No argument comes
    /// from the model.
    Run { argv: Vec<String> },
}

fn simple_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-' || b == b'.'
        })
        && !s.starts_with('.')
}

impl Act {
    /// Whether the model must supply text for this act.
    pub fn needs_text(&self) -> bool {
        matches!(
            self,
            Act::Reply | Act::Journal | Act::Message { .. } | Act::WriteFile { .. }
        )
    }

    /// A charter may only name acts that stay inside the world directory.
    pub fn is_well_formed(&self) -> bool {
        match self {
            Act::None | Act::Reply | Act::Journal => true,
            Act::Message { recipient } => simple_name(recipient),
            Act::WriteFile { name } => simple_name(name),
            Act::Run { argv } => {
                !argv.is_empty()
                    && argv.len() <= 16
                    && Path::new(&argv[0]).is_absolute()
                    && argv
                        .iter()
                        .all(|a| !a.is_empty() && a.len() <= 256 && !a.contains('\0'))
            }
        }
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        match self {
            Act::None => commit::frame(&[b"none"]),
            Act::Reply => commit::frame(&[b"reply"]),
            Act::Journal => commit::frame(&[b"journal"]),
            Act::Message { recipient } => commit::frame(&[b"message", recipient.as_bytes()]),
            Act::WriteFile { name } => commit::frame(&[b"write_file", name.as_bytes()]),
            Act::Run { argv } => {
                let parts: Vec<&[u8]> = argv.iter().map(|a| a.as_bytes()).collect();
                commit::frame(&[b"run", &commit::frame(&parts)])
            }
        }
    }

    /// Text that this act can carry: present when needed, bounded, printable.
    pub fn accepts_text(&self, text: &str) -> bool {
        let printable = text
            .chars()
            .all(|c| !c.is_control() || c == '\n' || c == '\t');
        text.len() <= MAX_TEXT && printable && (!self.needs_text() || !text.trim().is_empty())
    }
}

/// One file an act wrote, by its path inside the world and the hash of its bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub path: String,
    #[serde(with = "crate::hexser::array")]
    pub hash: Hash,
}

/// What a committed step did in the world, under the device key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectReceipt {
    pub sequence: u64,
    #[serde(with = "crate::hexser::array")]
    pub decision_commit: Hash,
    pub outputs: Vec<Output>,
    #[serde(with = "crate::hexser::bytes")]
    pub signature: Vec<u8>,
}

fn output_bytes(outputs: &[Output]) -> Vec<u8> {
    let parts: Vec<Vec<u8>> = outputs
        .iter()
        .map(|o| commit::frame(&[o.path.as_bytes(), &o.hash]))
        .collect();
    let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
    commit::frame(&refs)
}

impl EffectReceipt {
    pub fn commitment(&self) -> Hash {
        commit::receipt_commit(
            self.sequence,
            &self.decision_commit,
            &output_bytes(&self.outputs),
        )
    }

    pub fn sign(
        sequence: u64,
        decision_commit: Hash,
        outputs: Vec<Output>,
        device: &Keypair,
    ) -> Result<Self, AdapterError> {
        let mut r = EffectReceipt {
            sequence,
            decision_commit,
            outputs,
            signature: vec![],
        };
        r.signature = sphincs::sphincs_sign(&device.secret, &r.commitment())
            .map_err(|e| AdapterError::Dsm(e.to_string()))?;
        Ok(r)
    }

    pub fn is_signed_by(&self, device_public: &[u8]) -> bool {
        matches!(
            sphincs::sphincs_verify(device_public, &self.commitment(), &self.signature),
            Ok(true)
        )
    }
}

pub fn file_hash(bytes: &[u8]) -> Hash {
    commit::world_file_hash(bytes)
}

fn io(e: std::io::Error) -> AdapterError {
    AdapterError::Effect(e.to_string())
}

/// Write `bytes` to `relative` inside `world`, through a temporary file so a crash leaves either
/// the old bytes or the new ones.
fn put(world: &Path, relative: &str, bytes: &[u8]) -> Result<Output, AdapterError> {
    let path = world.join(relative);
    let dir = path
        .parent()
        .ok_or_else(|| AdapterError::Effect("no parent directory".to_string()))?;
    std::fs::create_dir_all(dir).map_err(io)?;
    let temporary = dir.join(format!(".{}.partial", std::process::id()));
    std::fs::write(&temporary, bytes).map_err(io)?;
    std::fs::rename(&temporary, &path).map_err(io)?;
    Ok(Output {
        path: relative.to_string(),
        hash: file_hash(bytes),
    })
}

/// The operating-system confinement a command runs in: no network, no write anywhere, and reads
/// limited to system files and the world. With no way to confine a command on this system, no
/// command runs at all.
#[cfg(target_os = "macos")]
fn confined(world: &Path, argv: &[String]) -> Result<Command, AdapterError> {
    let world = world.canonicalize().map_err(io)?;
    let world = world
        .to_str()
        .filter(|w| !w.contains('"') && !w.contains('\\'))
        .ok_or_else(|| {
            AdapterError::Effect("the world's path cannot be named in a profile".to_string())
        })?;
    let profile = format!(
        "(version 1)(deny default)(import \"bsd.sb\")(allow process-exec process-fork)\
         (allow file-read* (literal \"/\") (subpath \"/usr\") (subpath \"/bin\") \
         (subpath \"/System\") (subpath \"/Library/Apple\") (subpath \"/private/var/db\") \
         (subpath \"/dev\") (subpath \"{world}\"))(allow file-write* (literal \"/dev/null\"))"
    );
    let mut c = Command::new("/usr/bin/sandbox-exec");
    c.arg("-p").arg(profile).args(argv).current_dir(world);
    Ok(c)
}

#[cfg(target_os = "linux")]
fn confined(world: &Path, argv: &[String]) -> Result<Command, AdapterError> {
    const BWRAP: &str = "/usr/bin/bwrap";
    if !Path::new(BWRAP).exists() {
        return Err(AdapterError::Effect(
            "commands run only under bubblewrap, and it is not installed".to_string(),
        ));
    }
    let world = world.canonicalize().map_err(io)?;
    let mut c = Command::new(BWRAP);
    c.args([
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--clearenv",
    ])
    .args([
        "--ro-bind",
        "/usr",
        "/usr",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
    ]);
    for dir in ["/bin", "/sbin", "/lib", "/lib64", "/lib32"] {
        let path = Path::new(dir);
        if let Ok(target) = std::fs::read_link(path) {
            c.arg("--symlink").arg(target).arg(dir);
        } else if path.is_dir() {
            c.args(["--ro-bind", dir, dir]);
        }
    }
    c.arg("--ro-bind")
        .arg(&world)
        .arg("/world")
        .args(["--chdir", "/world", "--"])
        .args(argv);
    Ok(c)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn confined(_world: &Path, _argv: &[String]) -> Result<Command, AdapterError> {
    Err(AdapterError::Effect(
        "no way to confine a command is known on this system, so none runs".to_string(),
    ))
}

/// Whether the sandbox itself starts here. A sandbox that cannot start would make every command
/// look like it ran and failed, so the enforcer checks this first and refuses outright instead.
pub fn confinement_works(world: &Path) -> Result<(), AdapterError> {
    let out = confined(world, &["/usr/bin/true".to_string()])?
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(io)?;
    if out.status.success() {
        Ok(())
    } else {
        Err(AdapterError::Effect(format!(
            "the command sandbox could not start, so no command runs: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// Run `argv` confined to `world`, with an empty environment, for at most `limit`, keeping at
/// most [`MAX_OUTPUT`] bytes of what it prints. A confined command can write nothing, so running
/// it again after a stop has no effect beyond its own output.
pub fn run_command(
    world: &Path,
    argv: &[String],
    limit: Duration,
) -> Result<Vec<u8>, AdapterError> {
    confinement_works(world)?;
    let mut child = confined(world, argv)?
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(io)?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| AdapterError::Effect("no output".to_string()))?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout
            .by_ref()
            .take(MAX_OUTPUT as u64)
            .read_to_end(&mut out);
        out
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(io)? {
            break Some(status);
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut out = reader
        .join()
        .map_err(|_| AdapterError::Effect("the output reader failed".to_string()))?;
    match status {
        Some(s) => {
            out.extend_from_slice(format!("\n[exit {}]\n", s.code().unwrap_or(-1)).as_bytes())
        }
        None => out.extend_from_slice(b"\n[stopped after the time limit]\n"),
    }
    Ok(out)
}

/// Carry out `act` for the step at `sequence`, with the model's `text`. Called only by the
/// enforcer, only for a step it has committed.
pub fn perform(
    world: &Path,
    sequence: u64,
    act: &Act,
    text: &str,
) -> Result<Vec<Output>, AdapterError> {
    if !act.is_well_formed() || !act.accepts_text(text) {
        return Err(AdapterError::Effect(
            "an act the charter could not have signed".to_string(),
        ));
    }
    let n = format!("{sequence:06}");
    let body = text.as_bytes();
    let outputs = match act {
        Act::None => vec![],
        Act::Reply => vec![put(world, &format!("replies/{n}.txt"), body)?],
        Act::Journal => vec![put(world, &format!("journal/{n}.md"), body)?],
        Act::Message { recipient } => {
            vec![put(world, &format!("outbox/{recipient}/{n}.txt"), body)?]
        }
        Act::WriteFile { name } => vec![put(world, &format!("files/{name}"), body)?],
        Act::Run { argv } => vec![put(
            world,
            &format!("runs/{n}.out"),
            &run_command(world, argv, RUN_LIMIT)?,
        )?],
    };
    Ok(outputs)
}

/// Remove temporary files an interrupted write left behind. Only the enforcer writes here.
pub fn sweep_partial(world: &Path) -> Result<usize, AdapterError> {
    let mut found = Vec::new();
    if world.exists() {
        walk(world, world, &mut found).map_err(io)?;
    }
    let mut removed = 0;
    for (_, path) in found {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') && name.ends_with(".partial") {
            std::fs::remove_file(&path).map_err(io)?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, root, out)?;
        } else {
            let relative = path
                .strip_prefix(root)
                .map_err(std::io::Error::other)?
                .to_string_lossy()
                .to_string();
            out.push((relative, path));
        }
    }
    Ok(())
}

/// Every file in the world must be what the latest receipt touching its path says it is, and
/// nothing may be there that no receipt explains.
pub fn check_world(world: &Path, receipts: &[EffectReceipt]) -> Result<usize, AdapterError> {
    let mut expected = std::collections::BTreeMap::new();
    for r in receipts {
        for o in &r.outputs {
            expected.insert(o.path.clone(), o.hash);
        }
    }
    let mut found = Vec::new();
    if world.exists() {
        walk(world, world, &mut found).map_err(io)?;
    }
    for (relative, path) in &found {
        let bytes = std::fs::read(path).map_err(io)?;
        match expected.get(relative) {
            Some(hash) if *hash == file_hash(&bytes) => {}
            Some(_) => {
                return Err(AdapterError::Effect(format!(
                    "{relative} differs from what its receipt says was written"
                )))
            }
            None => {
                return Err(AdapterError::Effect(format!(
                    "{relative} is in the world but no committed action wrote it"
                )))
            }
        }
    }
    if found.len() != expected.len() {
        return Err(AdapterError::Effect(
            "a file a receipt says was written is missing".to_string(),
        ));
    }
    Ok(found.len())
}
