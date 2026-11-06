//! A live view of a chain, in your browser. It reads the store, the world, the charter and the
//! public keys, and holds no key. Every verdict on the page is worked out again on each change.
//!
//! ethical-console --store DIR --world DIR --authority PUBLIC --device PUBLIC [--port 8770]
//!
//! The store carries every charter that governed it.
//!
//! It listens on 127.0.0.1 only.

use ethical_autonomy_adapter::console::build;
use ethical_autonomy_adapter::keys::read_public_key;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::SystemTime;

const PAGE: &str = include_str!("../console.html");

struct View {
    store: PathBuf,
    world: PathBuf,
    authority: Vec<u8>,
    device: Vec<u8>,
    /// The last page of state, and the store's size and time it was built from.
    cached: Option<((u64, Option<SystemTime>), String)>,
}

impl View {
    fn state(&mut self) -> String {
        let stamp = std::fs::metadata(self.store.join("chain.jsonl"))
            .map(|m| (m.len(), m.modified().ok()))
            .unwrap_or((0, None));
        if let Some((s, json)) = &self.cached {
            if *s == stamp {
                return json.clone();
            }
        }
        let console = build(&self.store, &self.world, &self.authority, &self.device);
        let json =
            serde_json::to_string(&console).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
        self.cached = Some((stamp, json.clone()));
        json
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

fn serve(view: &mut View, mut stream: TcpStream) -> std::io::Result<()> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let path = line.split_whitespace().nth(1).unwrap_or("");
    match (line.starts_with("GET "), path) {
        (true, "/") => respond(&mut stream, "200 OK", "text/html; charset=utf-8", PAGE),
        (true, "/state.json") => {
            let json = view.state();
            respond(&mut stream, "200 OK", "application/json", &json)
        }
        _ => respond(&mut stream, "404 Not Found", "text/plain", "not found"),
    }
}

fn run() -> Result<(), String> {
    let mut a = std::env::args().skip(1);
    let (mut store, mut world, mut authority, mut device) = (None, None, None, None);
    let mut port = 8770u16;
    while let Some(flag) = a.next() {
        let value = a.next().ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--store" => store = Some(PathBuf::from(value)),
            "--world" => world = Some(PathBuf::from(value)),
            "--authority" => authority = Some(PathBuf::from(value)),
            "--device" => device = Some(PathBuf::from(value)),
            "--port" => port = value.parse().map_err(|_| "bad --port")?,
            other => return Err(format!("unknown flag {other}")),
        }
    }
    let mut view = View {
        store: store.ok_or("--store is required")?,
        world: world.ok_or("--world is required")?,
        authority: read_public_key(&authority.ok_or("--authority is required")?)
            .map_err(|e| e.to_string())?,
        device: read_public_key(&device.ok_or("--device is required")?)
            .map_err(|e| e.to_string())?,
        cached: None,
    };
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("listen: {e}"))?;
    let address = listener.local_addr().map_err(|e| e.to_string())?;
    println!("console at http://{address}/");
    std::io::stdout().flush().ok();
    for stream in listener.incoming().flatten() {
        if let Err(e) = serve(&mut view, stream) {
            eprintln!("request failed: {e}");
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
