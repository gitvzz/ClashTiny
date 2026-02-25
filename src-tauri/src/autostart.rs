use std::fs;
use std::path::PathBuf;
use std::process::Command;

const PLIST_LABEL: &str = "com.clash-tiny.app";

fn plist_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join("Library").join("LaunchAgents")
}

fn plist_path() -> PathBuf {
    plist_dir().join(format!("{PLIST_LABEL}.plist"))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn build_plist() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("获取程序路径失败: {e}"))?;
    let exe_path = xml_escape(&exe.to_string_lossy());

    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{PLIST_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe_path}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>
"#
    ))
}

/// Check whether the LaunchAgent plist actually exists on disk.
pub fn is_enabled() -> bool {
    plist_path().exists()
}

/// Create the LaunchAgent plist and load it into the current session.
pub fn enable() -> Result<(), String> {
    let dir = plist_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("创建 LaunchAgents 目录失败: {e}"))?;

    let path = plist_path();
    let content = build_plist()?;
    fs::write(&path, &content).map_err(|e| format!("写入 plist 失败: {e}"))?;

    let output = Command::new("launchctl")
        .args(["load", &path.to_string_lossy()])
        .output()
        .map_err(|e| format!("执行 launchctl load 失败: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("already loaded") && !stderr.contains("Already loaded") {
            let _ = fs::remove_file(&path);
            return Err(format!("launchctl load 失败: {}", stderr.trim()));
        }
    }

    println!("[ClashTiny] Auto-start enabled: {}", path.display());
    Ok(())
}

/// Unload the LaunchAgent from the current session and remove the plist.
pub fn disable() -> Result<(), String> {
    let path = plist_path();
    if !path.exists() {
        return Ok(());
    }

    let _ = Command::new("launchctl")
        .args(["unload", &path.to_string_lossy()])
        .output();

    fs::remove_file(&path).map_err(|e| format!("删除 plist 失败: {e}"))?;

    println!("[ClashTiny] Auto-start disabled");
    Ok(())
}
