use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

const DEFAULT_OVERRIDE: &str = include_str!("../resources/override.yaml");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    #[serde(default)]
    pub proxy_mode: ProxyMode,
    #[serde(default)]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub auto_start: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    SystemProxy,
    Tun,
    None,
}

impl Default for ProxyMode {
    fn default() -> Self {
        ProxyMode::None
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            proxy_mode: ProxyMode::None,
            active_profile: Option::None,
            auto_start: false,
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Cannot determine home directory")
        .join(".config")
        .join("clash-tiny")
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.yaml")
}

pub fn override_file() -> PathBuf {
    config_dir().join("override.yaml")
}

pub fn profiles_dir() -> PathBuf {
    config_dir().join("profiles")
}

pub fn state_file() -> PathBuf {
    config_dir().join("state.json")
}

pub fn ensure_dirs() {
    fs::create_dir_all(config_dir()).ok();
    fs::create_dir_all(profiles_dir()).ok();
}

/// Startup: ensure config.yaml and override.yaml exist.
/// If config.yaml is missing and there is an active profile in state.json,
/// rebuild config.yaml from profile + override (preserving proxy rules).
/// Otherwise fall back to override.yaml as a minimal config.
pub fn ensure_default_files() {
    let overrides = override_file();
    if !overrides.exists() {
        fs::write(&overrides, DEFAULT_OVERRIDE).ok();
        println!("[ClashTiny] Created default override.yaml");
    }

    let config = config_file();
    if !config.exists() {
        if let Some(profile_name) = load_state().active_profile {
            let profile_path = profiles_dir().join(format!("{}.yaml", profile_name));
            if profile_path.exists() {
                if let Ok(profile_content) = fs::read_to_string(&profile_path) {
                    if let Ok(merged) = apply_override(&profile_content) {
                        fs::write(&config, &merged).ok();
                        println!(
                            "[ClashTiny] Rebuilt config.yaml from profile '{}' + override",
                            profile_name
                        );
                        return;
                    }
                }
            }
        }
        let override_content =
            fs::read_to_string(&overrides).unwrap_or_else(|_| DEFAULT_OVERRIDE.to_string());
        fs::write(&config, override_content).ok();
        println!("[ClashTiny] Created default config.yaml from override.yaml");
    }
}

/// Apply override.yaml on top of a base YAML config (subscription).
/// Implements 4-level merge strategy per docs/配置.md:
///   Level 1: mandatory fields (forced override)
///   Level 2: tun (override base + whitelist from subscription)
///   Level 3: dns (subscription priority + locked fields + fill missing)
///   Level 4: geox-url (override base + subscription overwrites)
pub fn apply_override(base_yaml: &str) -> Result<String, String> {
    let override_content = fs::read_to_string(override_file())
        .map_err(|e| format!("读取 override.yaml 失败: {e}"))?;

    let mut base: serde_yaml::Value = serde_yaml::from_str(base_yaml)
        .map_err(|e| format!("解析基础配置失败: {e}"))?;
    let overrides: serde_yaml::Value = serde_yaml::from_str(&override_content)
        .map_err(|e| format!("解析覆盖配置失败: {e}"))?;

    let base_map = base
        .as_mapping_mut()
        .ok_or("基础配置不是有效的 YAML mapping")?;
    let override_map = overrides
        .as_mapping()
        .ok_or("覆盖配置不是有效的 YAML mapping")?;

    let is_tun = load_state().proxy_mode == ProxyMode::Tun;

    merge_level1_mandatory(base_map, override_map);
    merge_level2_tun(base_map, override_map, is_tun);
    merge_level3_dns(base_map, override_map);
    merge_level4_geox(base_map, override_map);

    serde_yaml::to_string(&base).map_err(|e| format!("序列化合并配置失败: {e}"))
}

fn ykey(s: &str) -> serde_yaml::Value {
    serde_yaml::Value::String(s.to_string())
}

/// Level 1: force override mandatory fields (program depends on these exact values).
fn merge_level1_mandatory(
    base: &mut serde_yaml::Mapping,
    overrides: &serde_yaml::Mapping,
) {
    const MANDATORY_KEYS: &[&str] = &[
        "mixed-port",
        "external-controller",
        "secret",
        "external-ui",
        "external-ui-url",
    ];
    for &k in MANDATORY_KEYS {
        let key = ykey(k);
        if let Some(val) = overrides.get(&key) {
            base.insert(key, val.clone());
        }
    }
}

/// Level 2: tun — override as base, only whitelist fields from subscription.
/// Injects tun.enable based on current runtime proxy mode.
fn merge_level2_tun(
    base: &mut serde_yaml::Mapping,
    overrides: &serde_yaml::Mapping,
    is_tun: bool,
) {
    const TUN_WHITELIST: &[&str] = &["mtu", "udp-timeout"];
    let tun_key = ykey("tun");

    let mut tun = overrides
        .get(&tun_key)
        .and_then(|v| v.as_mapping())
        .cloned()
        .unwrap_or_default();

    if let Some(sub_tun) = base.get(&tun_key).and_then(|v| v.as_mapping()) {
        for &field in TUN_WHITELIST {
            let k = ykey(field);
            if let Some(val) = sub_tun.get(&k) {
                tun.insert(k, val.clone());
            }
        }
    }

    tun.insert(ykey("enable"), serde_yaml::Value::Bool(is_tun));

    base.insert(tun_key, serde_yaml::Value::Mapping(tun));
}

/// Level 3: dns — subscription priority; lock enable/listen; fill missing from override.
fn merge_level3_dns(
    base: &mut serde_yaml::Mapping,
    overrides: &serde_yaml::Mapping,
) {
    let dns_key = ykey("dns");

    let mut result = if let Some(sub_dns) = base.get(&dns_key).and_then(|v| v.as_mapping()) {
        let mut r = sub_dns.clone();
        // Fill missing fields from override's dns defaults
        if let Some(ov_dns) = overrides.get(&dns_key).and_then(|v| v.as_mapping()) {
            for (k, v) in ov_dns {
                if !r.contains_key(k) {
                    r.insert(k.clone(), v.clone());
                }
            }
        }
        r
    } else {
        // No DNS in subscription — use override's dns entirely
        overrides
            .get(&dns_key)
            .and_then(|v| v.as_mapping())
            .cloned()
            .unwrap_or_default()
    };

    // Lock mandatory dns fields
    result.insert(ykey("enable"), serde_yaml::Value::Bool(true));
    result.insert(
        ykey("listen"),
        serde_yaml::Value::String("0.0.0.0:1053".to_string()),
    );

    base.insert(dns_key, serde_yaml::Value::Mapping(result));
}

/// Level 4: geox-url — override as base, subscription fields overwrite.
fn merge_level4_geox(
    base: &mut serde_yaml::Mapping,
    overrides: &serde_yaml::Mapping,
) {
    let geox_key = ykey("geox-url");

    let mut geox = overrides
        .get(&geox_key)
        .and_then(|v| v.as_mapping())
        .cloned()
        .unwrap_or_default();

    if let Some(sub_geox) = base.get(&geox_key).and_then(|v| v.as_mapping()) {
        for (k, v) in sub_geox {
            geox.insert(k.clone(), v.clone());
        }
    }

    base.insert(geox_key, serde_yaml::Value::Mapping(geox));
}

pub fn load_state() -> AppState {
    let path = state_file();
    if path.exists() {
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str::<AppState>(&data) {
                return state;
            }
        }
    }
    AppState::default()
}

pub fn save_state(state: &AppState) {
    if let Ok(json) = serde_json::to_string_pretty(state) {
        atomic_write(&state_file(), json.as_bytes()).ok();
    }
}

/// Write to a temp file then atomically rename, preventing partial writes.
pub fn atomic_write(target: &std::path::Path, data: &[u8]) -> Result<(), String> {
    let dir = target.parent().unwrap_or(std::path::Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .map_err(|e| format!("创建临时文件失败: {e}"))?;
    tmp.write_all(data)
        .map_err(|e| format!("写入临时文件失败: {e}"))?;
    tmp.persist(target)
        .map_err(|e| format!("原子替换失败: {e}"))?;
    Ok(())
}

/// Toggle tun.enable in config.yaml. All other TUN/DNS fields come from override.yaml.
pub fn set_tun_enabled(enabled: bool) -> Result<(), String> {
    let path = config_file();
    let content = fs::read_to_string(&path).map_err(|e| format!("读取 config.yaml 失败: {e}"))?;

    let mut doc: serde_yaml::Value =
        serde_yaml::from_str(&content).map_err(|e| format!("解析 config.yaml 失败: {e}"))?;

    if let Some(map) = doc.as_mapping_mut() {
        let tun_key = serde_yaml::Value::String("tun".into());
        let tun_section = map
            .entry(tun_key)
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));

        if let Some(tun_map) = tun_section.as_mapping_mut() {
            tun_map.insert(
                serde_yaml::Value::String("enable".into()),
                serde_yaml::Value::Bool(enabled),
            );
        }
    }

    let yaml = serde_yaml::to_string(&doc).map_err(|e| format!("序列化 config 失败: {e}"))?;
    atomic_write(&path, yaml.as_bytes()).map_err(|e| format!("写入 config.yaml 失败: {e}"))?;
    println!("[ClashTiny] tun.enable set to {enabled}");
    Ok(())
}

/// Locate the mihomo binary. Checks dev path, then exe-relative path.
/// For production with Tauri resource_dir fallback, use core_manager::find_mihomo_bin().
pub fn find_mihomo_bin_path() -> Result<PathBuf, String> {
    let arch = std::env::consts::ARCH;
    let sidecar_name = format!("mihomo-{}-apple-darwin", arch);

    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("bin")
        .join(&sidecar_name);
    if dev_path.exists() {
        return Ok(dev_path);
    }

    if let Ok(exe) = std::env::current_exe() {
        let real_exe = exe.canonicalize().unwrap_or(exe);
        if let Some(dir) = real_exe.parent() {
            let prod_path = dir.join(&sidecar_name);
            if prod_path.exists() {
                return Ok(prod_path);
            }
        }
    }

    Err(format!("找不到 mihomo 二进制: {sidecar_name}"))
}

/// List profiles in profiles/, excluding temp files (prefixed with _).
pub fn list_profiles() -> Vec<(String, String)> {
    let dir = profiles_dir();
    let mut profiles = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "yaml" || e == "yml") {
                let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                if stem.starts_with('_') {
                    continue;
                }
                profiles.push((stem.clone(), stem));
            }
        }
    }
    profiles.sort_by(|a, b| a.0.cmp(&b.0));
    profiles
}
