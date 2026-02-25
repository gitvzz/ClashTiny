use serde::{Deserialize, Serialize};
use std::fs;
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
                    if let Ok(merged) = merge_profile_with_override(&profile_content) {
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

/// Merge a profile YAML string with override.yaml (override wins for shared keys).
pub fn merge_profile_with_override(profile_yaml: &str) -> Result<String, String> {
    let override_content = fs::read_to_string(override_file())
        .map_err(|e| format!("读取 override.yaml 失败: {e}"))?;

    let mut profile: serde_yaml::Value = serde_yaml::from_str(profile_yaml)
        .map_err(|e| format!("解析订阅配置失败: {e}"))?;

    let overrides: serde_yaml::Value = serde_yaml::from_str(&override_content)
        .map_err(|e| format!("解析 override.yaml 失败: {e}"))?;

    if let (Some(p_map), Some(o_map)) = (profile.as_mapping_mut(), overrides.as_mapping()) {
        for (key, value) in o_map {
            p_map.insert(key.clone(), value.clone());
        }
    }

    serde_yaml::to_string(&profile).map_err(|e| format!("序列化合并配置失败: {e}"))
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
        fs::write(state_file(), json).ok();
    }
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
    fs::write(&path, yaml).map_err(|e| format!("写入 config.yaml 失败: {e}"))?;
    println!("[ClashTiny] tun.enable set to {enabled}");
    Ok(())
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
