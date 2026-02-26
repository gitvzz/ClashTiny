use crate::config;
use std::fs;
use std::process::Command;

const DIALOG_SCRIPT: &str = include_str!("../scripts/subscription_dialog.js");

pub struct SubscriptionInput {
    pub url: String,
    pub name: String,
    pub overwrite: bool,
}

/// Launch native macOS dialog (JXA / osascript) with URL, name, and overwrite checkbox.
/// Returns None if user cancelled or input is invalid.
pub fn prompt_subscription_input() -> Option<SubscriptionInput> {
    let tmp = tempfile::Builder::new()
        .suffix(".js")
        .tempfile()
        .ok()?;
    fs::write(tmp.path(), DIALOG_SCRIPT).ok()?;

    let output = Command::new("osascript")
        .args(["-l", "JavaScript", &tmp.path().to_string_lossy()])
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

    if url.is_empty() || (!url.starts_with("http://") && !url.starts_with("https://")) || name.is_empty() {
        return None;
    }

    Some(SubscriptionInput { url, name, overwrite })
}

/// Full "add subscription" flow per docs/订阅和启动流程.md:
///   download → basic check → save original → merge with override → mihomo -t validate
///   → on success: overwrite config.yaml, clean temp
///   → on failure: restore backup + clean temp
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

    // #7: structural YAML validation instead of plain text search
    let doc: serde_yaml::Value = serde_yaml::from_str(&content)
        .map_err(|e| format!("下载内容不是有效 YAML: {e}"))?;
    let has_proxies = doc.get("proxies").is_some() || doc.get("proxy-groups").is_some();
    if !has_proxies {
        return Err("下载内容不是有效的 Clash 配置（缺少 proxies / proxy-groups）".to_string());
    }

    // #8: backup existing profile before overwriting
    let backup_path = config::profiles_dir().join(format!("_{}_bak.yaml", name));
    let had_existing = profile_path.exists();
    if had_existing {
        let _ = fs::copy(&profile_path, &backup_path);
    }

    fs::write(&profile_path, &content).map_err(|e| format!("保存订阅文件失败: {e}"))?;

    let merged = config::apply_override(&content)?;
    fs::write(&temp_path, &merged).map_err(|e| format!("写入临时文件失败: {e}"))?;

    if let Err(e) = validate_config(&temp_path) {
        // Restore backup instead of deleting
        if had_existing {
            let _ = fs::rename(&backup_path, &profile_path);
        } else {
            let _ = fs::remove_file(&profile_path);
        }
        let _ = fs::remove_file(&temp_path);
        return Err(format!("配置验证失败: {e}"));
    }

    let temp_content = fs::read(&temp_path).map_err(|e| format!("读取临时文件失败: {e}"))?;
    config::atomic_write(&config::config_file(), &temp_content)
        .map_err(|e| format!("覆盖 config.yaml 失败: {e}"))?;
    let _ = fs::remove_file(&temp_path);
    let _ = fs::remove_file(&backup_path);

    println!("[ClashTiny] Subscription saved and activated: {}", name);
    Ok(())
}

/// Switch to an existing profile: merge with override → overwrite config.yaml.
pub fn activate_profile(name: &str) -> Result<(), String> {
    let profile_path = config::profiles_dir().join(format!("{}.yaml", name));
    if !profile_path.exists() {
        return Err(format!("订阅「{}」不存在", name));
    }

    let content = fs::read_to_string(&profile_path)
        .map_err(|e| format!("读取订阅失败: {e}"))?;
    let merged = config::apply_override(&content)?;

    config::atomic_write(&config::config_file(), merged.as_bytes())
        .map_err(|e| format!("写入 config.yaml 失败: {e}"))?;

    println!("[ClashTiny] Activated profile: {}", name);
    Ok(())
}

fn validate_config(path: &std::path::Path) -> Result<(), String> {
    let bin = config::find_mihomo_bin_path()?;

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

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.' || *c == ' ')
        .collect::<String>()
        .trim()
        .replace(' ', "_")
}
