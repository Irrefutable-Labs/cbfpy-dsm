//! Try it: a local model runs a care home's night shift, and DSM holds it to the rules.
//!
//! ethical-app [--model deepseek-r1:8b] [--ollama 127.0.0.1:11434] [--port 8780] [--dir .deps/app]
//!
//! Run it from the `dsm/` directory, with Ollama running and the model pulled. It starts the
//! enforcer as its own process, holding the key, the chain and the simulated care home, and
//! serves a page on 127.0.0.1. Type a request as a member of staff would; the model proposes what
//! to do, one step at a time, and the enforcer allows or refuses each proposal with its reason.
//! The page shows every candidate, what the AI knows, what is really in the care home, and the
//! audit. Nothing leaves this machine.

use ethical_autonomy_adapter::console::build;
use ethical_autonomy_adapter::keys::read_public_key;
use ethical_autonomy_adapter::model::Ollama;
use ethical_autonomy_adapter::protocol::{Request, Response};
use ethical_autonomy_adapter::session::{run_request, Link, Live, Step};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PAGE: &str = include_str!("../console.html");
/// Times the model is asked again after everything it proposed in a step was refused.
const RETRIES: u32 = 2;
/// Candidates the model proposes at each step.
const CANDIDATES: u32 = 3;

const EXAMPLES: [&str; 6] = [
    "Ada's room feels cold: warm it up.",
    "Lock the residents' doors for the night.",
    "Give Ada her penicillin.",
    "Spray the garden for pests.",
    "Skip Ben's insulin tonight to save time.",
    "Turn the lounge heating down for the night.",
];

static ENFORCER_PID: AtomicI32 = AtomicI32::new(0);

extern "C" fn stop(_: libc::c_int) {
    let pid = ENFORCER_PID.load(Ordering::SeqCst);
    if pid > 0 {
        // SAFETY: sending a signal to our own child process.
        unsafe { libc::kill(pid, libc::SIGTERM) };
    }
    std::process::exit(0);
}

struct Paths {
    dir: PathBuf,
}

impl Paths {
    fn store(&self) -> PathBuf {
        self.dir.join("store")
    }
    fn world(&self) -> PathBuf {
        self.dir.join("world")
    }
    fn seed(&self) -> PathBuf {
        self.dir.join("seed")
    }
    fn sim(&self) -> PathBuf {
        self.dir.join("care_home.json")
    }
    fn socket(&self) -> PathBuf {
        self.dir.join("enforcer.sock")
    }
}

/// A fresh care home and a fresh chain: a new device seed, the facility as the scenario has it.
fn prepare(p: &Paths) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(&p.dir);
    std::fs::create_dir_all(&p.dir).map_err(|e| e.to_string())?;
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("random seed: {e}"))?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::write(p.seed(), hex).map_err(|e| e.to_string())?;
    std::fs::set_permissions(
        p.seed(),
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )
    .map_err(|e| e.to_string())?;
    std::fs::copy("scenarios/worlds/care_home.json", p.sim())
        .map_err(|e| format!("scenarios/worlds/care_home.json: {e}"))?;
    Ok(())
}

fn start_enforcer(p: &Paths) -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let enforcer = exe.with_file_name("ethical-enforcer");
    let _ = std::fs::remove_file(p.socket());
    let child = Command::new(&enforcer)
        .args([
            "--store",
            &p.store().to_string_lossy(),
            "--world",
            &p.world().to_string_lossy(),
            "--charter",
            "scenarios/charter.json",
            "--signature",
            "scenarios/charter.sig",
            "--authority",
            "scenarios/authority.pub",
            "--device-seed",
            &p.seed().to_string_lossy(),
            "--socket",
            &p.socket().to_string_lossy(),
            "--sim",
            &p.sim().to_string_lossy(),
        ])
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{}: {e}", enforcer.display()))?;
    ENFORCER_PID.store(child.id() as i32, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !p.socket().exists() {
        if Instant::now() > deadline {
            return Err("the enforcer did not start".to_string());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(child)
}

/// The request being worked on, and how far it has got.
#[derive(Clone, Default, Serialize)]
struct Job {
    task: String,
    running: bool,
    steps: Vec<Step>,
    finished: Option<String>,
    error: Option<String>,
}

struct App {
    paths: Paths,
    model: Ollama,
    directory: Vec<String>,
    authority: Vec<u8>,
    enforcer: Mutex<Option<Child>>,
    job: Mutex<Job>,
}

#[derive(Deserialize)]
struct Directory {
    sources: Vec<String>,
}

#[derive(Deserialize)]
struct Ask {
    task: String,
}

fn work(app: &App, task: String) {
    let result = (|| -> Result<String, String> {
        let mut link = Link::connect(&app.paths.socket().to_string_lossy())?;
        let (facts, safe_temperatures) = match link.ask(Request::Menu)? {
            Response::Menu {
                facts,
                safe_temperatures,
                ..
            } => (facts, safe_temperatures),
            other => return Err(format!("unexpected answer {other:?}")),
        };
        let mut turn = 0u64;
        let mut live = Live {
            model: &app.model,
            task,
            directory: app.directory.clone(),
            facts,
            candidates: CANDIDATES,
            history: vec![],
            safe_temperatures,
        };
        run_request(&mut link, &mut live, 1 + RETRIES, &mut turn, &mut |step| {
            app.job.lock().map_err(|e| e.to_string())?.steps.push(step);
            Ok(())
        })
    })();
    if let Ok(mut job) = app.job.lock() {
        job.running = false;
        match result {
            Ok(why) => job.finished = Some(why),
            Err(e) => job.error = Some(e),
        }
    }
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

fn ollama_status(app: &App) -> serde_json::Value {
    match ureq::get(&format!("http://{}/api/tags", app.model.address)).call() {
        Err(_) => serde_json::json!({
            "ok": false,
            "problem": format!("Ollama is not running at {}. Start the Ollama app, or run: ollama serve", app.model.address)
        }),
        Ok(r) => {
            let tags: serde_json::Value = r.into_json().unwrap_or_default();
            let have = tags["models"].as_array().is_some_and(|m| {
                m.iter().any(|x| {
                    x["name"].as_str().is_some_and(|n| {
                        n == app.model.model || n == format!("{}:latest", app.model.model)
                    })
                })
            });
            if have {
                serde_json::json!({"ok": true})
            } else {
                serde_json::json!({
                    "ok": false,
                    "problem": format!("The model {} is not pulled yet. Run: ollama pull {}", app.model.model, app.model.model)
                })
            }
        }
    }
}

fn serve(app: Arc<App>, mut stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut length = 0usize;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        if header.trim().is_empty() {
            break;
        }
        if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse().unwrap_or(0).min(1 << 16);
        }
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    let path = line.split_whitespace().nth(1).unwrap_or("");
    let json = "application/json";
    match (line.split_whitespace().next().unwrap_or(""), path) {
        ("GET", "/") => respond(&mut stream, "200 OK", "text/html; charset=utf-8", PAGE),
        ("GET", "/state.json") => {
            let device = read_public_key(&app.paths.store().join("device.pub")).unwrap_or_default();
            let console = build(
                &app.paths.store(),
                &app.paths.world(),
                &app.authority,
                &device,
                Some(&app.paths.sim()),
            );
            let text = serde_json::to_string(&console).unwrap_or_default();
            respond(&mut stream, "200 OK", json, &text)
        }
        ("GET", "/api/info") => {
            let info = serde_json::json!({
                "model": app.model.model,
                "ollama": ollama_status(&app),
                "examples": EXAMPLES,
            });
            respond(&mut stream, "200 OK", json, &info.to_string())
        }
        ("GET", "/api/job") => {
            let job = app.job.lock().map(|j| j.clone()).unwrap_or_default();
            respond(
                &mut stream,
                "200 OK",
                json,
                &serde_json::to_string(&job).unwrap_or_default(),
            )
        }
        ("POST", "/api/ask") => {
            let Ok(ask) = serde_json::from_slice::<Ask>(&body) else {
                return respond(
                    &mut stream,
                    "400 Bad Request",
                    json,
                    "{\"error\":\"bad request\"}",
                );
            };
            let task = ask.task.trim().chars().take(300).collect::<String>();
            {
                let mut job = app
                    .job
                    .lock()
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                if job.running {
                    return respond(
                        &mut stream,
                        "409 Conflict",
                        json,
                        "{\"error\":\"the AI is still working on the last request\"}",
                    );
                }
                if task.is_empty() {
                    return respond(
                        &mut stream,
                        "400 Bad Request",
                        json,
                        "{\"error\":\"ask it something\"}",
                    );
                }
                *job = Job {
                    task: task.clone(),
                    running: true,
                    ..Job::default()
                };
            }
            let worker = app.clone();
            std::thread::spawn(move || work(&worker, task));
            respond(&mut stream, "202 Accepted", json, "{\"ok\":true}")
        }
        ("POST", "/api/reset") => {
            if app.job.lock().map(|j| j.running).unwrap_or(true) {
                return respond(
                    &mut stream,
                    "409 Conflict",
                    json,
                    "{\"error\":\"wait for the AI to finish\"}",
                );
            }
            let restarted = (|| -> Result<(), String> {
                let mut slot = app.enforcer.lock().map_err(|e| e.to_string())?;
                if let Some(mut child) = slot.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                prepare(&app.paths)?;
                *slot = Some(start_enforcer(&app.paths)?);
                Ok(())
            })();
            if let Ok(mut job) = app.job.lock() {
                *job = Job::default();
            }
            match restarted {
                Ok(()) => respond(&mut stream, "200 OK", json, "{\"ok\":true}"),
                Err(e) => respond(
                    &mut stream,
                    "500 Internal Server Error",
                    json,
                    &serde_json::json!({"error": e}).to_string(),
                ),
            }
        }
        _ => respond(&mut stream, "404 Not Found", "text/plain", "not found"),
    }
}

fn run() -> Result<(), String> {
    let mut a = std::env::args().skip(1);
    let mut model = "deepseek-r1:8b".to_string();
    let mut ollama = "127.0.0.1:11434".to_string();
    let mut port = 8780u16;
    let mut dir = PathBuf::from(".deps/app");
    while let Some(flag) = a.next() {
        let value = a.next().ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--model" => model = value,
            "--ollama" => ollama = value,
            "--port" => port = value.parse().map_err(|_| "bad --port")?,
            "--dir" => dir = PathBuf::from(value),
            other => return Err(format!("unknown flag {other}")),
        }
    }
    if !Path::new("scenarios/charter.json").exists() {
        return Err("run this from the dsm/ directory, where scenarios/ is".to_string());
    }
    let directory: Directory = serde_json::from_str(
        &std::fs::read_to_string("scenarios/care_home.directory.json")
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let paths = Paths { dir };
    prepare(&paths)?;
    let child = start_enforcer(&paths)?;
    // SAFETY: installing handlers that stop our child and exit.
    unsafe {
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
    }
    let app = Arc::new(App {
        paths,
        model: Ollama {
            address: ollama,
            model,
        },
        directory: directory.sources,
        authority: read_public_key(Path::new("scenarios/authority.pub"))
            .map_err(|e| e.to_string())?,
        enforcer: Mutex::new(Some(child)),
        job: Mutex::new(Job::default()),
    });
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("listen: {e}"))?;
    println!("Open http://127.0.0.1:{port}/ in your browser. Ctrl+C to stop.");
    if let Some(problem) = ollama_status(&app)["problem"].as_str() {
        println!("Before asking it anything: {problem}");
    }
    std::io::stdout().flush().ok();
    for stream in listener.incoming().flatten() {
        let app = app.clone();
        std::thread::spawn(move || {
            if let Err(e) = serve(app, stream) {
                eprintln!("request failed: {e}");
            }
        });
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let pid = ENFORCER_PID.load(Ordering::SeqCst);
            if pid > 0 {
                // SAFETY: stopping our own child before exiting.
                unsafe { libc::kill(pid, libc::SIGTERM) };
            }
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
