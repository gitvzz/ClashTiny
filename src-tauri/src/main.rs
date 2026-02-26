#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod config;
mod core_manager;
mod helper_manager;
mod proxy_manager;
mod subscription;
mod uninstall;

use config::{AppState, ProxyMode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{
    image::Image,
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::TrayIconBuilder,
    Manager,
};

struct AppData {
    state: Mutex<AppState>,
    mihomo_error: AtomicBool,
    switching: AtomicBool, // #4: prevent concurrent mode switches
}

fn lock_state(m: &Mutex<AppState>) -> std::sync::MutexGuard<'_, AppState> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn main() {
    config::ensure_dirs();
    config::ensure_default_files();
    let initial_state = config::load_state();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppData {
            state: Mutex::new(initial_state),
            mihomo_error: AtomicBool::new(false),
            switching: AtomicBool::new(false),
        })
        .manage(core_manager::CoreState::new())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            {
                let data = app.state::<AppData>();
                let mut state = lock_state(&data.state);
                let actual = autostart::is_enabled();
                if state.auto_start != actual {
                    println!(
                        "[ClashTiny] auto_start state mismatch: state.json={}, plist={}. Syncing to plist.",
                        state.auto_start, actual
                    );
                    state.auto_start = actual;
                    config::save_state(&state);
                }
            }

            build_tray(app.handle())?;
            if let Err(e) = core_manager::start_mihomo(app.handle()) {
                eprintln!("[ClashTiny] Failed to start mihomo: {e}");
            }
            let data = app.state::<AppData>();
            let current = lock_state(&data.state).clone();
            match current.proxy_mode {
                ProxyMode::SystemProxy => {
                    if let Err(e) = proxy_manager::enable_system_proxy() {
                        eprintln!("[ClashTiny] Failed to restore system proxy: {e}");
                    }
                }
                ProxyMode::Tun => {
                    if helper_manager::is_helper_installed() && helper_manager::is_helper_running() {
                        core_manager::stop_mihomo(app.handle());
                        let _ = config::set_tun_enabled(true);
                        // #3: use shared find_mihomo_bin instead of hardcoded dev path
                        match core_manager::find_mihomo_bin(app.handle()) {
                            Ok(bin) => {
                                let cdir = config::config_dir().to_string_lossy().to_string();
                                let _ = helper_manager::start_mihomo_via_helper(
                                    &bin.to_string_lossy(),
                                    &cdir,
                                );
                            }
                            Err(e) => eprintln!("[ClashTiny] Cannot find mihomo for TUN: {e}"),
                        }
                    } else {
                        eprintln!("[ClashTiny] TUN mode saved but helper not running, falling back to None");
                        let mut s = lock_state(&data.state);
                        s.proxy_mode = ProxyMode::None;
                        config::save_state(&s);
                    }
                }
                ProxyMode::None => {}
            }

            start_watchdog(app.handle().clone());

            let warmup_handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                let _ = rebuild_tray(&warmup_handle);
            });

            Ok(())
        })
        .on_window_event(|_window, _event| {})
        .run(tauri::generate_context!())
        .expect("error while running Clash Tiny");
}

// ---------------------------------------------------------------------------
// Tray
// ---------------------------------------------------------------------------

fn get_tray_icon(mode: &ProxyMode, is_error: bool) -> Image<'static> {
    let bytes: &[u8] = if is_error {
        include_bytes!("../icons/tray-e@2x.png")
    } else {
        match mode {
            ProxyMode::None => include_bytes!("../icons/tray-n@2x.png"),
            ProxyMode::SystemProxy => include_bytes!("../icons/tray-s@2x.png"),
            ProxyMode::Tun => include_bytes!("../icons/tray-t@2x.png"),
        }
    };
    Image::from_bytes(bytes).expect("failed to load tray icon")
}

fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    let data = app.state::<AppData>();
    let mode = lock_state(&data.state).proxy_mode.clone();
    let is_error = data.mihomo_error.load(Ordering::Relaxed);
    let icon = get_tray_icon(&mode, is_error);

    let _tray = TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("Clash Tiny")
        .on_menu_event(move |app, event| {
            handle_menu_event(app, event.id().as_ref());
        })
        .build(app)?;

    Ok(())
}

fn build_menu(app: &tauri::AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let state = app.state::<AppData>();
    let current = lock_state(&state.state).clone();

    let sub_submenu = build_subscription_submenu(app, &current)?;

    let mode_system = CheckMenuItemBuilder::with_id("mode_system_proxy", "系统代理")
        .checked(current.proxy_mode == ProxyMode::SystemProxy)
        .build(app)?;
    let mode_tun = CheckMenuItemBuilder::with_id("mode_tun", "透明代理")
        .checked(current.proxy_mode == ProxyMode::Tun)
        .build(app)?;
    let mode_none = CheckMenuItemBuilder::with_id("mode_none", "无")
        .checked(current.proxy_mode == ProxyMode::None)
        .build(app)?;

    let proxy_submenu = SubmenuBuilder::with_id(app, "proxy_mode", "代理模式")
        .item(&mode_system)
        .item(&mode_tun)
        .item(&mode_none)
        .build()?;

    let dashboard = MenuItemBuilder::with_id("open_dashboard", "控制面板").build(app)?;

    let auto_start = CheckMenuItemBuilder::with_id("auto_start", "开机自启")
        .checked(current.auto_start)
        .build(app)?;
    let global_override = MenuItemBuilder::with_id("global_override", "全局覆盖")
        .enabled(false)
        .build(app)?;
    let open_config_dir = MenuItemBuilder::with_id("open_config_dir", "打开配置目录").build(app)?;

    let uninstall = MenuItemBuilder::with_id("uninstall", "卸载").build(app)?;

    let settings_submenu = SubmenuBuilder::with_id(app, "settings", "设置")
        .item(&auto_start)
        .item(&global_override)
        .separator()
        .item(&open_config_dir)
        .separator()
        .item(&uninstall)
        .build()?;

    let about = MenuItemBuilder::with_id("about", "关于").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;

    MenuBuilder::new(app)
        .item(&sub_submenu)
        .item(&proxy_submenu)
        .item(&dashboard)
        .item(&settings_submenu)
        .separator()
        .item(&about)
        .item(&quit)
        .build()
}

fn build_subscription_submenu(
    app: &tauri::AppHandle,
    current: &AppState,
) -> tauri::Result<tauri::menu::Submenu<tauri::Wry>> {
    let mut submenu = SubmenuBuilder::with_id(app, "subscriptions", "订阅列表");

    let runtime_config = MenuItemBuilder::with_id("open_runtime_config", "运行期配置").build(app)?;
    submenu = submenu.item(&runtime_config).separator();

    let profiles = config::list_profiles();
    if profiles.is_empty() {
        let empty = MenuItemBuilder::with_id("no_profiles", "（config）")
            .enabled(false)
            .build(app)?;
        submenu = submenu.item(&empty);
    } else {
        for (stem, display) in &profiles {
            let is_active = current.active_profile.as_deref() == Some(stem.as_str());
            let item = CheckMenuItemBuilder::with_id(format!("profile_{}", stem), display.as_str())
                .checked(is_active)
                .build(app)?;
            submenu = submenu.item(&item);
        }
    }

    let add_sub = MenuItemBuilder::with_id("add_subscription", "添加订阅...").build(app)?;
    submenu = submenu.separator().item(&add_sub);

    submenu.build()
}

// ---------------------------------------------------------------------------
// Menu events
// ---------------------------------------------------------------------------

fn handle_menu_event(app: &tauri::AppHandle, event_id: &str) {
    match event_id {
        "open_runtime_config" => {
            let _ = open::that(config::config_file().to_string_lossy().as_ref());
        }

        "add_subscription" => {
            let app = app.clone();
            std::thread::spawn(move || {
                let Some(input) = subscription::prompt_subscription_input() else {
                    return;
                };

                if let Err(e) =
                    subscription::download_and_save(&input.url, &input.name, input.overwrite)
                {
                    show_error_dialog(&format!("订阅失败：{}", e));
                    return;
                }

                if let Err(e) = core_manager::reload_mihomo(&app) {
                    eprintln!("[ClashTiny] Reload after subscribe failed: {e}");
                }

                {
                    let data = app.state::<AppData>();
                    let mut state = lock_state(&data.state);
                    state.active_profile = Some(input.name);
                    config::save_state(&state);
                }

                let _ = rebuild_tray(&app);
            });
        }

        id if id.starts_with("profile_") => {
            let profile_name = id.strip_prefix("profile_").unwrap_or("").to_string();
            let app = app.clone();
            std::thread::spawn(move || {
                switch_profile(&app, &profile_name);
            });
        }

        "mode_system_proxy" => set_proxy_mode(app, ProxyMode::SystemProxy),
        "mode_tun" => set_proxy_mode(app, ProxyMode::Tun),
        "mode_none" => set_proxy_mode(app, ProxyMode::None),

        "open_dashboard" => {
            let _ = open::that(
                "http://127.0.0.1:9090/ui/?host=127.0.0.1&hostname=127.0.0.1&port=9090&secret=ClashTiny",
            );
        }

        "auto_start" => {
            let data = app.state::<AppData>();
            let current = lock_state(&data.state).auto_start;
            let result = if current {
                autostart::disable()
            } else {
                autostart::enable()
            };
            match result {
                Ok(()) => {
                    let mut state = lock_state(&data.state);
                    state.auto_start = !current;
                    config::save_state(&state);
                }
                Err(e) => {
                    eprintln!("[ClashTiny] Auto-start toggle failed: {e}");
                    show_error_dialog(&format!("设置开机自启失败：{e}"));
                }
            }
            let _ = rebuild_tray(app);
        }

        "open_config_dir" => {
            let _ = open::that(config::config_dir().to_string_lossy().as_ref());
        }

        "uninstall" => {
            let app = app.clone();
            std::thread::spawn(move || {
                if uninstall::confirm_uninstall() {
                    uninstall::perform_uninstall(&app);
                }
            });
        }

        "about" => {
            show_about_dialog();
        }

        // #5: use app.exit() instead of std::process::exit() to run destructors
        "quit" => {
            let data = app.state::<AppData>();
            let mode = lock_state(&data.state).proxy_mode.clone();
            match mode {
                ProxyMode::SystemProxy => {
                    let _ = proxy_manager::disable_system_proxy();
                }
                ProxyMode::Tun => {
                    let _ = helper_manager::stop_mihomo_via_helper();
                    let _ = config::set_tun_enabled(false);
                }
                ProxyMode::None => {}
            }
            core_manager::stop_mihomo(app);
            app.exit(0);
        }

        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

struct SwitchingGuard<'a>(&'a AtomicBool);
impl Drop for SwitchingGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn set_proxy_mode(app: &tauri::AppHandle, mode: ProxyMode) {
    let data = app.state::<AppData>();

    // #4: prevent concurrent mode switches
    if data.switching.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        eprintln!("[ClashTiny] Mode switch already in progress, ignoring");
        let _ = rebuild_tray(app);
        return;
    }

    let old = lock_state(&data.state).proxy_mode.clone();

    if old == mode {
        data.switching.store(false, Ordering::SeqCst);
        let _ = rebuild_tray(app);
        return;
    }

    match mode {
        ProxyMode::None => {
            teardown_mode(app, &old);
            save_and_rebuild(app, ProxyMode::None);
            data.switching.store(false, Ordering::SeqCst);
        }
        ProxyMode::SystemProxy => {
            if let Err(e) = proxy_manager::enable_system_proxy() {
                eprintln!("[ClashTiny] Enable system proxy failed: {e}");
                show_error_dialog(&format!("开启系统代理失败：{}", e));
                data.switching.store(false, Ordering::SeqCst);
                let _ = rebuild_tray(app);
                return;
            }
            teardown_mode(app, &old);
            save_and_rebuild(app, ProxyMode::SystemProxy);
            data.switching.store(false, Ordering::SeqCst);
        }
        ProxyMode::Tun => {
            let app_clone = app.clone();
            let old_clone = old.clone();
            std::thread::spawn(move || {
                let data = app_clone.state::<AppData>();
                // A: Drop guard ensures switching is reset even on panic
                let _guard = SwitchingGuard(&data.switching);
                match setup_tun(&app_clone) {
                    Ok(()) => {
                        teardown_mode(&app_clone, &old_clone);
                        save_and_rebuild(&app_clone, ProxyMode::Tun);
                    }
                    Err(e) => {
                        show_error_dialog(&format!("开启透明代理失败：{}", e));
                        let _ = rebuild_tray(&app_clone);
                    }
                }
            });
        }
    }
}

fn save_and_rebuild(app: &tauri::AppHandle, mode: ProxyMode) {
    let data = app.state::<AppData>();
    {
        let mut state = lock_state(&data.state);
        state.proxy_mode = mode;
        config::save_state(&state);
    }
    let _ = rebuild_tray(app);
}

fn teardown_mode(app: &tauri::AppHandle, mode: &ProxyMode) {
    match mode {
        ProxyMode::SystemProxy => {
            let _ = proxy_manager::disable_system_proxy();
        }
        ProxyMode::Tun => {
            let _ = helper_manager::stop_mihomo_via_helper();
            let _ = config::set_tun_enabled(false);
            if let Err(e) = core_manager::start_mihomo(app) {
                eprintln!("[ClashTiny] Failed to restart sidecar after TUN teardown: {e}");
            }
        }
        ProxyMode::None => {}
    }
}

fn setup_tun(app: &tauri::AppHandle) -> Result<(), String> {
    if !helper_manager::is_helper_installed() {
        println!("[ClashTiny] Helper not installed, prompting installation...");
        helper_manager::install_helper()?;
    }

    let mut helper_ready = false;
    for i in 0..10 {
        if helper_manager::is_helper_running() {
            helper_ready = true;
            break;
        }
        println!("[ClashTiny] Waiting for helper to start... ({}/10)", i + 1);
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    if !helper_ready {
        return Err("Helper 服务未能启动，请检查 /tmp/clash-tiny-helper.log".to_string());
    }

    core_manager::stop_mihomo(app);
    config::set_tun_enabled(true)?;

    // #3: use shared find_mihomo_bin instead of hardcoded dev path
    let bin_path = core_manager::find_mihomo_bin(app)?;
    let bin_str = bin_path.to_string_lossy().to_string();
    let config_dir = config::config_dir().to_string_lossy().to_string();

    let resp = helper_manager::start_mihomo_via_helper(&bin_str, &config_dir)?;
    if resp.code != 0 {
        let _ = config::set_tun_enabled(false);
        let _ = core_manager::start_mihomo(app);
        return Err(format!("Helper 启动 Mihomo 失败: {}", resp.message));
    }

    println!("[ClashTiny] TUN mode enabled via helper: {}", resp.message);
    Ok(())
}

fn switch_profile(app: &tauri::AppHandle, name: &str) {
    if let Err(e) = subscription::activate_profile(name) {
        show_error_dialog(&format!("切换订阅失败：{}", e));
        return;
    }

    if let Err(e) = core_manager::reload_mihomo(app) {
        eprintln!("[ClashTiny] Reload after switch failed: {e}");
    }

    {
        let data = app.state::<AppData>();
        let mut state = lock_state(&data.state);
        state.active_profile = Some(name.to_string());
        config::save_state(&state);
    }
    let _ = rebuild_tray(app);
}

fn rebuild_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id("main") {
        let menu = build_menu(app)?;
        tray.set_menu(Some(menu))?;

        let data = app.state::<AppData>();
        let mode = lock_state(&data.state).proxy_mode.clone();
        let is_error = data.mihomo_error.load(Ordering::Relaxed);
        let icon = get_tray_icon(&mode, is_error);
        tray.set_icon(Some(icon))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Watchdog: periodic health check + auto-restart
// ---------------------------------------------------------------------------

fn start_watchdog(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(10));

        let mut consecutive_failures: u32 = 0;
        const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);
        const FAILURE_THRESHOLD: u32 = 3;
        const WAKE_TOLERANCE: std::time::Duration = std::time::Duration::from_secs(20);

        loop {
            let before = std::time::Instant::now();
            std::thread::sleep(CHECK_INTERVAL);
            let elapsed = before.elapsed();

            let data = app.state::<AppData>();

            if elapsed > CHECK_INTERVAL + WAKE_TOLERANCE {
                println!(
                    "[Watchdog] Wake from sleep detected (slept {}s, expected {}s)",
                    elapsed.as_secs(),
                    CHECK_INTERVAL.as_secs()
                );
                std::thread::sleep(std::time::Duration::from_secs(3));
                handle_wake_recovery(&app, &data);
                consecutive_failures = 0;
                continue;
            }

            if core_manager::is_api_healthy() {
                if consecutive_failures > 0 || data.mihomo_error.load(Ordering::Relaxed) {
                    data.mihomo_error.store(false, Ordering::Relaxed);
                    let _ = rebuild_tray(&app);
                }
                consecutive_failures = 0;
                continue;
            }

            consecutive_failures += 1;
            eprintln!(
                "[Watchdog] API unhealthy ({}/{})",
                consecutive_failures, FAILURE_THRESHOLD
            );

            if consecutive_failures >= 2 && !data.mihomo_error.load(Ordering::Relaxed) {
                data.mihomo_error.store(true, Ordering::Relaxed);
                let _ = rebuild_tray(&app);
            }

            if consecutive_failures < FAILURE_THRESHOLD {
                continue;
            }

            eprintln!("[Watchdog] Threshold reached, attempting recovery...");
            consecutive_failures = 0;

            let mode = lock_state(&data.state).proxy_mode.clone();

            match mode {
                // #3: use shared find_mihomo_bin for watchdog TUN recovery
                ProxyMode::Tun => {
                    if helper_manager::is_helper_running() {
                        match core_manager::find_mihomo_bin(&app) {
                            Ok(bin) => {
                                let bin_str = bin.to_string_lossy().to_string();
                                let cdir = config::config_dir().to_string_lossy().to_string();
                                let _ = helper_manager::stop_mihomo_via_helper();
                                let _ = config::set_tun_enabled(true);
                                match helper_manager::start_mihomo_via_helper(&bin_str, &cdir) {
                                    Ok(r) if r.code == 0 => {
                                        println!("[Watchdog] TUN Mihomo restarted: {}", r.message);
                                        data.mihomo_error.store(false, Ordering::Relaxed);
                                        let _ = rebuild_tray(&app);
                                    }
                                    Ok(r) => eprintln!("[Watchdog] Restart failed: {}", r.message),
                                    Err(e) => eprintln!("[Watchdog] Restart error: {e}"),
                                }
                            }
                            Err(e) => eprintln!("[Watchdog] Cannot find mihomo binary: {e}"),
                        }
                    }
                }
                _ => {
                    core_manager::stop_mihomo(&app);
                    match core_manager::start_mihomo(&app) {
                        Ok(()) => {
                            println!("[Watchdog] Sidecar restarted");
                            data.mihomo_error.store(false, Ordering::Relaxed);
                            let _ = rebuild_tray(&app);
                        }
                        Err(e) => eprintln!("[Watchdog] Sidecar restart failed: {e}"),
                    }
                }
            }
        }
    });
}

fn handle_wake_recovery(app: &tauri::AppHandle, data: &tauri::State<'_, AppData>) {
    let mode = lock_state(&data.state).proxy_mode.clone();
    println!("[Watchdog] Wake recovery for mode: {:?}", mode);

    match mode {
        ProxyMode::Tun => {
            if helper_manager::is_helper_running() {
                let _ = helper_manager::stop_mihomo_via_helper();
                std::thread::sleep(std::time::Duration::from_secs(1));

                let _ = config::set_tun_enabled(true);
                match core_manager::find_mihomo_bin(app) {
                    Ok(bin) => {
                        let cdir = config::config_dir().to_string_lossy().to_string();
                        match helper_manager::start_mihomo_via_helper(&bin.to_string_lossy(), &cdir) {
                            Ok(r) if r.code == 0 => {
                                println!("[Watchdog] TUN recovered after wake: {}", r.message);
                                data.mihomo_error.store(false, Ordering::Relaxed);
                            }
                            Ok(r) => {
                                eprintln!("[Watchdog] TUN recovery failed: {}", r.message);
                                data.mihomo_error.store(true, Ordering::Relaxed);
                            }
                            Err(e) => {
                                eprintln!("[Watchdog] TUN recovery error: {e}");
                                data.mihomo_error.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[Watchdog] Cannot find mihomo binary: {e}");
                        data.mihomo_error.store(true, Ordering::Relaxed);
                    }
                }
            } else {
                eprintln!("[Watchdog] Helper not running after wake, cannot recover TUN");
                data.mihomo_error.store(true, Ordering::Relaxed);
            }
            let _ = rebuild_tray(app);
        }
        ProxyMode::SystemProxy => {
            if let Err(e) = proxy_manager::enable_system_proxy() {
                eprintln!("[Watchdog] System proxy recovery failed: {e}");
            }
            core_manager::stop_mihomo(app);
            match core_manager::start_mihomo(app) {
                Ok(()) => {
                    println!("[Watchdog] Sidecar recovered after wake");
                    data.mihomo_error.store(false, Ordering::Relaxed);
                }
                Err(e) => {
                    eprintln!("[Watchdog] Sidecar recovery failed: {e}");
                    data.mihomo_error.store(true, Ordering::Relaxed);
                }
            }
            let _ = rebuild_tray(app);
        }
        ProxyMode::None => {
            core_manager::stop_mihomo(app);
            let _ = core_manager::start_mihomo(app);
        }
    }
}

// #9: proper escaping for AppleScript double-quoted strings
fn escape_for_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn show_error_dialog(msg: &str) {
    let escaped = escape_for_applescript(msg);
    let script = format!(
        r#"tell application "System Events" to activate
display dialog "{}" with title "Clash Tiny" buttons {{"确定"}} default button "确定" with icon stop"#,
        escaped
    );
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output();
}

fn show_about_dialog() {
    let app_version = env!("CARGO_PKG_VERSION");
    // B: sanitize version string to prevent JXA injection
    let mihomo_version = core_manager::get_mihomo_version()
        .unwrap_or_else(|| "未运行".to_string())
        .replace('\\', "")
        .replace('"', "'")
        .replace(')', "");

    let script = format!(
        r#"ObjC.import('Cocoa');

var app = $.NSApplication.sharedApplication;
app.setActivationPolicy($.NSApplicationActivationPolicyAccessory);

var alert = $.NSAlert.alloc.init;
alert.messageText = $("Clash Tiny");
alert.informativeText = $("版本  {app_version}\n内核  {mihomo_version}");
alert.addButtonWithTitle($("好"));
alert.addButtonWithTitle($("访问 GitHub"));
alert.window.level = $.NSFloatingWindowLevel;

app.activateIgnoringOtherApps(true);
var response = alert.runModal;
response == 1001 ? "github" : "ok";"#
    );
    std::thread::spawn(move || {
        let output = std::process::Command::new("osascript")
            .arg("-l")
            .arg("JavaScript")
            .arg("-e")
            .arg(&script)
            .output();
        if let Ok(out) = output {
            let result = String::from_utf8_lossy(&out.stdout);
            if result.trim() == "github" {
                let _ = open::that("https://github.com/gitvzz/ClashTiny");
            }
        }
    });
}
