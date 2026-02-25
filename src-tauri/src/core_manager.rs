use std::process::{Child, Command};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::Manager;

const API_BASE: &str = "http://127.0.0.1:9090";
const API_SECRET: &str = "ClashTiny";

fn api_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("failed to build HTTP client")
    })
}

pub struct CoreState {
    pub child: Mutex<Option<Child>>,
}

impl CoreState {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
        }
    }
}

pub fn get_mihomo_version() -> Option<String> {
    let resp = api_client()
        .get(format!("{API_BASE}/version"))
        .header("Authorization", format!("Bearer {API_SECRET}"))
        .send()
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().ok()?;
    let meta = json.get("meta")?.as_bool().unwrap_or(false);
    let version = json.get("version")?.as_str()?;
    if meta {
        Some(format!("Mihomo Meta {version}"))
    } else {
        Some(version.to_string())
    }
}

pub fn is_api_healthy() -> bool {
    api_client()
        .get(format!("{API_BASE}/version"))
        .header("Authorization", format!("Bearer {API_SECRET}"))
        .send()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Wait for Mihomo API to become responsive after startup.
/// Returns true if healthy within the timeout, false otherwise.
pub fn wait_until_healthy(max_attempts: u32, interval_ms: u64) -> bool {
    for i in 0..max_attempts {
        if is_api_healthy() {
            println!("[ClashTiny] Mihomo API healthy after {} checks", i + 1);
            return true;
        }
        std::thread::sleep(Duration::from_millis(interval_ms));
    }
    eprintln!("[ClashTiny] Mihomo API not healthy after {max_attempts} checks");
    false
}

pub fn find_mihomo_bin(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    if let Ok(p) = crate::config::find_mihomo_bin_path() {
        return Ok(p);
    }
    let arch = std::env::consts::ARCH;
    let sidecar_name = format!("mihomo-{}-apple-darwin", arch);
    if let Ok(resource_dir) = app.path().resource_dir() {
        let res_path = resource_dir.join(&sidecar_name);
        if res_path.exists() {
            return Ok(res_path);
        }
    }
    Err(format!(
        "Cannot find mihomo binary '{}'. Place it in src-tauri/bin/",
        sidecar_name
    ))
}

pub fn start_mihomo(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<CoreState>();
    let mut guard = state.child.lock().unwrap_or_else(|e| e.into_inner());

    // Clean up zombie process: child exists but has already exited
    if let Some(child) = guard.as_mut() {
        match child.try_wait() {
            Ok(Some(_status)) => {
                println!("[ClashTiny] Detected dead sidecar, cleaning up");
                guard.take();
            }
            Ok(None) => return Ok(()), // still running
            Err(_) => {
                guard.take();
            }
        }
    }

    let bin_path = find_mihomo_bin(app)?;
    let config_dir = crate::config::config_dir();

    let child = Command::new(&bin_path)
        .args(["-d", &config_dir.to_string_lossy()])
        .spawn()
        .map_err(|e| format!("Failed to spawn mihomo ({bin_path:?}): {e}"))?;

    println!("[ClashTiny] Mihomo started (pid: {})", child.id());
    *guard = Some(child);
    drop(guard);

    if !wait_until_healthy(10, 500) {
        eprintln!("[ClashTiny] Warning: Mihomo started but API not responding");
    }

    Ok(())
}

pub fn stop_mihomo(app: &tauri::AppHandle) {
    let state = app.state::<CoreState>();
    let mut guard = state.child.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
        println!("[ClashTiny] Mihomo stopped");
    }
}

/// Hot-reload config via Mihomo REST API: PUT /configs with path to config.yaml.
/// If Mihomo is not running (sidecar child is None) AND we're not in TUN mode,
/// start it as a sidecar. In TUN mode, always use HTTP API since helper manages the process.
pub fn reload_mihomo(app: &tauri::AppHandle) -> Result<(), String> {
    let is_tun = crate::config::load_state().proxy_mode == crate::config::ProxyMode::Tun;
    reload_mihomo_inner(app, is_tun)
}

fn reload_mihomo_inner(app: &tauri::AppHandle, is_tun: bool) -> Result<(), String> {
    let config_path = crate::config::config_file();
    if !config_path.exists() {
        return Err("config.yaml not found".to_string());
    }

    if !is_tun {
        let state = app.state::<CoreState>();
        let guard = state.child.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            drop(guard);
            return start_mihomo(app);
        }
    }

    let abs_path = config_path.to_string_lossy().to_string();
    let body = serde_json::json!({ "path": abs_path }).to_string();

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| format!("HTTP client build failed: {e}"))?;

    let resp = client
        .put(format!("{API_BASE}/configs?force=true"))
        .header("Authorization", format!("Bearer {API_SECRET}"))
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .map_err(|e| format!("Reload request failed: {e}"))?;

    if resp.status().is_success() {
        println!("[ClashTiny] Reload response: {}", resp.status());
        Ok(())
    } else {
        Err(format!("Reload returned HTTP {}", resp.status()))
    }
}
