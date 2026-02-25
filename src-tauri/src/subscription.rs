use crate::config;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

pub struct SubscriptionInput {
    pub url: String,
    pub name: String,
    pub overwrite: bool,
}

/// Launch native macOS dialog (JXA / osascript) with URL, name, and overwrite checkbox.
/// Returns None if user cancelled or input is invalid.
pub fn prompt_subscription_input() -> Option<SubscriptionInput> {
    let script_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("subscription_dialog.js");

    let output = Command::new("osascript")
        .args(["-l", "JavaScript", script_path.to_str()?])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() || stdout == "undefined" {
        return None;
    }

    let parts: Vec<&str> = stdout.split('\u{1f}').collect();
    if parts.len() != 3 {
        return None;
    }

    let url = parts[0].trim().to_string();
    let name = sanitize_filename(parts[1].trim());
    let overwrite = parts[2].trim() == "1";

    if url.is_empty() || !url.starts_with("http") || name.is_empty() {
        return None;
    }

    Some(SubscriptionInput { url, name, overwrite })
}

/// Full "add subscription" flow per docs/订阅和启动流程.md:
///   download → basic check → save original → merge with override → mihomo -t validate
///   → on success: overwrite config.yaml, clean temp
///   → on failure: delete original + temp
pub fn download_and_save(url: &str, name: &str, overwrite: bool) -> Result<(), String> {
    let profile_path = config::profiles_dir().join(format!("{}.yaml", name));
    let temp_path = config::profiles_dir().join(format!("_{}.yaml", name));

    if !overwrite && profile_path.exists() {
        return Err(format!("订阅「{}」已存在，请勾选覆盖选项或使用其他名称", name));
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent("clash-tiny/0.1")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP 客户端错误: {e}"))?;

    let resp = client.get(url).send().map_err(|e| format!("下载失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let content = resp.text().map_err(|e| format!("读取响应失败: {e}"))?;

    if !content.contains("proxies") && !content.contains("proxy-groups") {
        return Err("下载内容不是有效的 Clash 配置（缺少 proxies / proxy-groups）".to_string());
    }

    fs::write(&profile_path, &content).map_err(|e| format!("保存订阅文件失败: {e}"))?;

    let merged = merge_with_override(&content)?;
    fs::write(&temp_path, &merged).map_err(|e| format!("写入临时文件失败: {e}"))?;

    if let Err(e) = validate_config(&temp_path) {
        let _ = fs::remove_file(&profile_path);
        let _ = fs::remove_file(&temp_path);
        return Err(format!("配置验证失败: {e}"));
    }

    fs::copy(&temp_path, config::config_file())
        .map_err(|e| format!("覆盖 config.yaml 失败: {e}"))?;
    let _ = fs::remove_file(&temp_path);

    println!("[ClashTiny] Subscription saved and activated: {}", name);
    Ok(())
}

/// Switch to an existing profile: merge with override → overwrite config.yaml.
/// No re-validation needed (already validated when first added).
pub fn activate_profile(name: &str) -> Result<(), String> {
    let profile_path = config::profiles_dir().join(format!("{}.yaml", name));
    if !profile_path.exists() {
        return Err(format!("订阅「{}」不存在", name));
    }

    let content = fs::read_to_string(&profile_path)
        .map_err(|e| format!("读取订阅失败: {e}"))?;
    let merged = merge_with_override(&content)?;

    fs::write(config::config_file(), &merged)
        .map_err(|e| format!("写入 config.yaml 失败: {e}"))?;

    println!("[ClashTiny] Activated profile: {}", name);
    Ok(())
}

fn merge_with_override(profile_yaml: &str) -> Result<String, String> {
    config::merge_profile_with_override(profile_yaml)
}

fn validate_config(path: &std::path::Path) -> Result<(), String> {
    let bin = find_mihomo_bin()?;

    let output = Command::new(&bin)
        .args(["-t", "-f", &path.to_string_lossy()])
        .output()
        .map_err(|e| format!("无法运行 mihomo: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let combined = if !stderr.is_empty() { stderr } else { stdout };
        let summary = combined
            .lines()
            .filter(|l| !l.trim().is_empty())
            .last()
            .unwrap_or("未知错误")
            .to_string();
        Err(summary)
    }
}

fn find_mihomo_bin() -> Result<PathBuf, String> {
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

    Err("找不到 mihomo 二进制文件".to_string())
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.' || *c == ' ')
        .collect::<String>()
        .trim()
        .replace(' ', "_")
}
