use std::fs;
use std::path::PathBuf;

use crate::{config, core_manager, helper_manager, proxy_manager};

pub fn confirm_uninstall() -> bool {
    let script = r#"tell application "System Events" to activate
display dialog "确定要卸载 Clash Tiny 吗？\n将清理所有配置、订阅、系统代理设置和 Helper 服务。此操作不可撤销。" with title "Clash Tiny 卸载" buttons {"取消", "确认卸载"} default button "取消" with icon caution"#;

    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.contains("确认卸载")
        }
        Err(_) => false,
    }
}

pub fn perform_uninstall(app: &tauri::AppHandle) {
    let mut errors: Vec<String> = Vec::new();

    // ① Disable system proxy
    println!("[Uninstall] Step 1: disable system proxy");
    if let Err(e) = proxy_manager::disable_system_proxy() {
        errors.push(format!("关闭系统代理: {e}"));
    }

    // ② Stop Mihomo process (both normal and TUN mode)
    println!("[Uninstall] Step 2: stop mihomo");
    let _ = helper_manager::stop_mihomo_via_helper();
    core_manager::stop_mihomo(app);

    // ③ Remove LaunchAgent plist
    println!("[Uninstall] Step 3: remove LaunchAgent");
    let launch_agent = dirs::home_dir()
        .map(|h| h.join("Library/LaunchAgents/com.clash-tiny.app.plist"));
    if let Some(path) = launch_agent {
        if path.exists() {
            if let Err(e) = fs::remove_file(&path) {
                errors.push(format!("删除开机自启 plist: {e}"));
            }
        }
    }

    // ④ Uninstall privileged Helper (requires admin password)
    println!("[Uninstall] Step 4: uninstall helper");
    if helper_manager::is_helper_installed() {
        if let Err(e) = uninstall_helper_with_admin() {
            errors.push(format!("清理 Helper 服务: {e}"));
        }
    }

    // ⑤ Remove temp files
    println!("[Uninstall] Step 5: remove temp files");
    let tmp_log = PathBuf::from("/tmp/clash-tiny-helper.log");
    if tmp_log.exists() {
        let _ = fs::remove_file(&tmp_log);
    }

    // ⑥ Remove config directory (~/.config/clash-tiny/)
    println!("[Uninstall] Step 6: remove config directory");
    let config_dir = config::config_dir();
    if config_dir.exists() {
        if let Err(e) = fs::remove_dir_all(&config_dir) {
            errors.push(format!("删除配置目录: {e}"));
        }
    }

    // ⑦ Remove .app bundle if running from one
    println!("[Uninstall] Step 7: check .app bundle");
    if let Some(app_bundle) = detect_app_bundle() {
        if let Err(e) = fs::remove_dir_all(&app_bundle) {
            errors.push(format!("删除应用程序: {e}"));
        }
    }

    // ⑧ Show result and exit
    if errors.is_empty() {
        show_result_dialog("Clash Tiny 已完全卸载。");
    } else {
        let detail = errors.join("\n• ");
        show_result_dialog(&format!(
            "卸载基本完成，以下项目未能清理：\n\n• {}\n\n可手动删除。",
            detail
        ));
    }

    app.exit(0);
}

fn uninstall_helper_with_admin() -> Result<(), String> {
    let script = r#"do shell script "launchctl unload /Library/LaunchDaemons/com.clash-tiny.helper.plist 2>/dev/null; rm -f /Library/PrivilegedHelperTools/com.clash-tiny.helper; rm -f /Library/LaunchDaemons/com.clash-tiny.helper.plist; rm -f /var/run/clash-tiny-helper.sock" with administrator privileges"#;

    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("执行 osascript 失败: {e}"))?;

    if output.status.success() {
        println!("[Uninstall] Helper uninstalled");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("User canceled") || stderr.contains("(-128)") {
            Err("用户取消了授权，Helper 未清理".to_string())
        } else {
            Err(format!("osascript 失败: {}", stderr.trim()))
        }
    }
}

fn detect_app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path_str = exe.to_string_lossy();
    let marker = ".app/Contents/MacOS/";
    let idx = path_str.find(marker)?;
    let app_path = &path_str[..idx + 4]; // include ".app"
    Some(PathBuf::from(app_path))
}

fn show_result_dialog(msg: &str) {
    let escaped = msg.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        r#"tell application "System Events" to activate
display dialog "{}" with title "Clash Tiny" buttons {{"确定"}} default button "确定""#,
        escaped
    );
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output();
}
