use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;

const HELPER_BIN_PATH: &str = "/Library/PrivilegedHelperTools/com.clash-tiny.helper";
const PLIST_PATH: &str = "/Library/LaunchDaemons/com.clash-tiny.helper.plist";
const SOCKET_PATH: &str = "/var/run/clash-tiny-helper.sock";

#[derive(Serialize)]
struct HelperRequest {
    command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    bin_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_dir: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct HelperResponse {
    pub code: i32,
    pub message: String,
}

pub fn is_helper_installed() -> bool {
    Path::new(HELPER_BIN_PATH).exists() && Path::new(PLIST_PATH).exists()
}

pub fn is_helper_running() -> bool {
    // Successfully connecting and getting any response means the helper daemon is alive
    send_command("status", None, None).is_ok()
}

/// Install helper via osascript prompting for admin password.
/// Returns Ok(()) if installation succeeds.
pub fn install_helper() -> Result<(), String> {
    let helper_src = find_helper_binary()?;
    let plist_src = find_helper_plist()?;
    let install_script = find_install_script()?;

    let script = format!(
        r#"do shell script "bash '{}' '{}' '{}'" with administrator privileges"#,
        install_script.to_string_lossy(),
        helper_src.to_string_lossy(),
        plist_src.to_string_lossy(),
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("Failed to run osascript: {e}"))?;

    if output.status.success() {
        println!("[ClashTiny] Helper installed successfully");
        std::thread::sleep(std::time::Duration::from_secs(1));
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("User canceled") || stderr.contains("(-128)") {
            Err("用户取消了授权".to_string())
        } else {
            Err(format!("安装失败: {}", stderr.trim()))
        }
    }
}

pub fn uninstall_helper() -> Result<(), String> {
    let uninstall_script = find_uninstall_script()?;

    let script = format!(
        r#"do shell script "bash '{}'" with administrator privileges"#,
        uninstall_script.to_string_lossy(),
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("Failed to run osascript: {e}"))?;

    if output.status.success() {
        println!("[ClashTiny] Helper uninstalled");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("卸载失败: {}", stderr.trim()))
    }
}

/// Start Mihomo via helper (root) with the given binary path and config dir.
pub fn start_mihomo_via_helper(bin_path: &str, config_dir: &str) -> Result<HelperResponse, String> {
    send_command(
        "start",
        Some(bin_path.to_string()),
        Some(config_dir.to_string()),
    )
}

/// Stop Mihomo via helper.
pub fn stop_mihomo_via_helper() -> Result<HelperResponse, String> {
    send_command("stop", None, None)
}

fn send_command(
    cmd: &str,
    bin_path: Option<String>,
    config_dir: Option<String>,
) -> Result<HelperResponse, String> {
    let mut stream = UnixStream::connect(SOCKET_PATH)
        .map_err(|e| format!("无法连接 Helper 服务 ({SOCKET_PATH}): {e}"))?;

    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .ok();

    let req = HelperRequest {
        command: cmd.to_string(),
        bin_path,
        config_dir,
    };
    let json = serde_json::to_string(&req).map_err(|e| format!("JSON 序列化失败: {e}"))?;

    writeln!(stream, "{json}").map_err(|e| format!("发送命令失败: {e}"))?;

    let reader = BufReader::new(stream);
    let line = reader
        .lines()
        .next()
        .ok_or("无响应".to_string())?
        .map_err(|e| format!("读取响应失败: {e}"))?;

    serde_json::from_str::<HelperResponse>(&line)
        .map_err(|e| format!("解析响应失败: {e}"))
}

fn find_helper_binary() -> Result<PathBuf, String> {
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
        .join("helper")
        .join("target")
        .join("release")
        .join("clash-tiny-helper");
    if dev_path.exists() {
        return Ok(dev_path);
    }

    let dev_debug = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
        .join("helper")
        .join("target")
        .join("debug")
        .join("clash-tiny-helper");
    if dev_debug.exists() {
        return Ok(dev_debug);
    }

    if let Ok(exe) = std::env::current_exe() {
        let real_exe = exe.canonicalize().unwrap_or(exe);
        if let Some(dir) = real_exe.parent() {
            let prod = dir.join("clash-tiny-helper");
            if prod.exists() {
                return Ok(prod);
            }
        }
    }

    Err("找不到 Helper 二进制文件，请先编译 helper 项目".to_string())
}

fn find_helper_plist() -> Result<PathBuf, String> {
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
        .join("helper")
        .join("scripts")
        .join("com.clash-tiny.helper.plist");
    if dev_path.exists() {
        return Ok(dev_path);
    }

    if let Ok(exe) = std::env::current_exe() {
        let real_exe = exe.canonicalize().unwrap_or(exe);
        if let Some(dir) = real_exe.parent() {
            let prod = dir.join("com.clash-tiny.helper.plist");
            if prod.exists() {
                return Ok(prod);
            }
        }
    }

    Err("找不到 Helper plist 文件".to_string())
}

fn find_install_script() -> Result<PathBuf, String> {
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
        .join("helper")
        .join("scripts")
        .join("install.sh");
    if dev_path.exists() {
        return Ok(dev_path);
    }

    if let Ok(exe) = std::env::current_exe() {
        let real_exe = exe.canonicalize().unwrap_or(exe);
        if let Some(dir) = real_exe.parent() {
            let prod = dir.join("install.sh");
            if prod.exists() {
                return Ok(prod);
            }
        }
    }

    Err("找不到 install.sh 脚本".to_string())
}

fn find_uninstall_script() -> Result<PathBuf, String> {
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
        .join("helper")
        .join("scripts")
        .join("uninstall.sh");
    if dev_path.exists() {
        return Ok(dev_path);
    }

    Err("找不到 uninstall.sh 脚本".to_string())
}
