use std::fs;
use std::path::PathBuf;

const PLIST_LABEL: &str = "com.clash-tiny.app";

fn plist_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("无法获取 HOME 目录")?;
    Ok(home.join("Library").join("LaunchAgents"))
}

fn plist_path() -> Result<PathBuf, String> {
    Ok(plist_dir()?.join(format!("{PLIST_LABEL}.plist")))
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

pub fn is_enabled() -> bool {
    plist_path().map(|p| p.exists()).unwrap_or(false)
}

pub fn enable() -> Result<(), String> {
    let dir = plist_dir()?;
    fs::create_dir_all(&dir).map_err(|e| format!("创建 LaunchAgents 目录失败: {e}"))?;

    let path = plist_path()?;
    let content = build_plist()?;
    fs::write(&path, &content).map_err(|e| format!("写入 plist 失败: {e}"))?;

    // Don't call `launchctl load` — it would immediately start a second instance
    // because RunAtLoad=true. macOS automatically loads plists from
    // ~/Library/LaunchAgents/ on next login.

    println!("[ClashTiny] Auto-start enabled: {}", path.display());
    Ok(())
}

pub fn disable() -> Result<(), String> {
    let path = plist_path()?;
    if !path.exists() {
        return Ok(());
    }

    fs::remove_file(&path).map_err(|e| format!("删除 plist 失败: {e}"))?;

    println!("[ClashTiny] Auto-start disabled");
    Ok(())
}
