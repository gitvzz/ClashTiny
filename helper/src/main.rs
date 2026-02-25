use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::Mutex;

const SOCKET_PATH: &str = "/var/run/clash-tiny-helper.sock";
const ALLOWED_BIN_PREFIX: &str = "mihomo";

static CHILD: Mutex<Option<Child>> = Mutex::new(None);

#[derive(Deserialize)]
struct Request {
    command: String,
    #[serde(default)]
    bin_path: Option<String>,
    #[serde(default)]
    config_dir: Option<String>,
}

#[derive(Serialize)]
struct Response {
    code: i32,
    message: String,
}

fn main() {
    if let Err(e) = std::fs::remove_file(SOCKET_PATH) {
        if e.kind() != std::io::ErrorKind::NotFound {
            eprintln!("[Helper] Cannot remove old socket: {e}");
        }
    }

    let listener = match UnixListener::bind(SOCKET_PATH) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[Helper] Failed to bind socket: {e}");
            std::process::exit(1);
        }
    };

    // Restrict socket to owner (root) + group (staff) only — no world access
    let _ = Command::new("chmod").args(["770", SOCKET_PATH]).status();
    // Allow members of 'staff' group (all normal macOS users) to connect
    let _ = Command::new("chown").args(["root:staff", SOCKET_PATH]).status();

    println!("[Helper] Listening on {SOCKET_PATH}");

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .ok();
                let cloned = match stream.try_clone() {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("[Helper] stream clone failed: {e}");
                        continue;
                    }
                };
                let reader = BufReader::new(cloned);
                for line in reader.lines() {
                    let line = match line {
                        Ok(l) => l,
                        Err(_) => break,
                    };
                    if line.is_empty() {
                        continue;
                    }
                    let resp = handle_request(&line);
                    let json = serde_json::to_string(&resp).unwrap_or_default();
                    let _ = writeln!(stream, "{json}");
                    break; // one request per connection
                }
            }
            Err(e) => eprintln!("[Helper] Connection error: {e}"),
        }
    }
}

fn handle_request(raw: &str) -> Response {
    let req: Request = match serde_json::from_str(raw) {
        Ok(r) => r,
        Err(e) => return Response { code: 1, message: format!("Invalid request: {e}") },
    };

    match req.command.as_str() {
        "start" => cmd_start(req),
        "stop" => cmd_stop(),
        "status" => cmd_status(),
        other => Response { code: 1, message: format!("Unknown command: {other}") },
    }
}

fn cmd_start(req: Request) -> Response {
    let bin_path = match req.bin_path {
        Some(p) if !p.is_empty() => p,
        _ => return Response { code: 1, message: "Missing bin_path".into() },
    };
    let config_dir = match req.config_dir {
        Some(p) if !p.is_empty() => p,
        _ => return Response { code: 1, message: "Missing config_dir".into() },
    };

    let bin = Path::new(&bin_path);
    if !bin.exists() {
        return Response { code: 1, message: format!("Binary not found: {bin_path}") };
    }
    let fname = bin.file_name().unwrap_or_default().to_string_lossy();
    if !fname.starts_with(ALLOWED_BIN_PREFIX) {
        return Response {
            code: 1,
            message: format!("Rejected: binary name '{}' not allowed", fname),
        };
    }

    // Stop existing process first
    stop_child();

    match Command::new(&bin_path).args(["-d", &config_dir]).spawn() {
        Ok(child) => {
            let pid = child.id();
            *CHILD.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
            println!("[Helper] Mihomo started (pid: {pid})");
            Response { code: 0, message: format!("Started pid={pid}") }
        }
        Err(e) => Response { code: 1, message: format!("Spawn failed: {e}") },
    }
}

fn cmd_stop() -> Response {
    stop_child();
    Response { code: 0, message: "Stopped".into() }
}

fn cmd_status() -> Response {
    let mut guard = CHILD.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_mut() {
        Some(child) => match child.try_wait() {
            Ok(Some(status)) => {
                let msg = format!("Exited with {}", status);
                guard.take();
                Response { code: 0, message: msg }
            }
            Ok(None) => Response {
                code: 0,
                message: format!("Running pid={}", child.id()),
            },
            Err(_) => {
                guard.take();
                Response { code: 0, message: "Not running".into() }
            }
        },
        None => Response { code: 0, message: "Not running".into() },
    }
}

fn stop_child() {
    let mut guard = CHILD.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
        println!("[Helper] Mihomo stopped");
    }
}
