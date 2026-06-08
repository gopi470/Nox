// lib.rs – Tauri command handlers + app setup
mod db;
mod bluetooth;
mod audio;
mod app_audio;
mod tracker;
mod spp;

use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::{ManagerExt, MacosLauncher};


use tracker::{Tracker, Snapshot};
use db::SessionRow;
use chrono::Local;

type TrackerState = Arc<Tracker>;

fn settings_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into()))
        .join("EarbudsTracker")
}

fn read_startup_enabled_from_disk() -> bool {
    let path = settings_dir().join("startup_enabled.txt");
    std::fs::read_to_string(&path)
        .ok()
        .map(|c| c.trim().eq_ignore_ascii_case("true") || c.trim() == "1")
        .unwrap_or(false)
}

fn write_startup_enabled_to_disk(enabled: bool) {
    let dir = settings_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("startup_enabled.txt"), if enabled { "true" } else { "false" });
}

// ── Tauri commands (called from JS frontend) ──────────────────────────────────

#[tauri::command]
fn get_snapshot(state: State<TrackerState>) -> Snapshot {
    state.get_snapshot()
}

#[tauri::command]
fn get_active_audio_apps(_state: State<TrackerState>) -> Vec<String> {
    crate::app_audio::get_all_app_peaks()
}

#[tauri::command]
fn get_sessions(state: State<TrackerState>) -> Vec<SessionRow> {
    state.get_recent_sessions()
}

#[tauri::command]
fn set_device_name(name: String, state: State<TrackerState>) {
    state.set_device_name(&name);
    if let Ok(appdata) = std::env::var("APPDATA") {
        let dir = std::path::PathBuf::from(appdata).join("EarbudsTracker");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("target_device.txt");
        let _ = std::fs::write(path, name);
    }
}

#[tauri::command]
fn get_battery_interval(state: State<TrackerState>) -> u64 {
    state.get_battery_interval_secs()
}

#[tauri::command]
fn set_battery_interval(secs: u64, state: State<TrackerState>) {
    state.set_battery_interval_secs(secs);
    if let Ok(appdata) = std::env::var("APPDATA") {
        let dir = std::path::PathBuf::from(appdata).join("EarbudsTracker");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("battery_interval.txt");
        let _ = std::fs::write(path, secs.to_string());
    }
}

#[tauri::command]
fn get_battery_step(state: State<TrackerState>) -> u8 {
    state.get_battery_step()
}

#[tauri::command]
fn set_battery_step(step: u8, state: State<TrackerState>) {
    state.set_battery_step(step);
    if let Ok(appdata) = std::env::var("APPDATA") {
        let dir = std::path::PathBuf::from(appdata).join("EarbudsTracker");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("battery_step.txt");
        let _ = std::fs::write(path, step.to_string());
    }
}

#[tauri::command]
fn reset_all(state: State<TrackerState>) {
    state.reset_all();
}

#[tauri::command]
fn get_daily_history(week_offset: i64) -> Vec<db::DailyStatsRow> {
    db::get_daily_history(week_offset)
}

#[tauri::command]
fn get_daily_history_bounds() -> db::DailyHistoryBounds {
    db::get_daily_history_bounds()
}

#[tauri::command]
fn get_query_log(state: State<TrackerState>) -> Vec<db::QueryLogRow> {
    state.get_query_log()
}

#[tauri::command]
fn export_all_data() -> ExportResult {
    let backup = db::export_backup();
    let exported_at = Local::now().to_rfc3339();
    let pretty = serde_json::to_string_pretty(&backup).unwrap_or_default();
    let filename = format!(
        "nox-backup-{}.json",
        Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    let primary_dir = std::path::PathBuf::from(appdata).join("EarbudsTracker").join("exports");
    let _ = std::fs::create_dir_all(&primary_dir);
    let primary_path = primary_dir.join(&filename);
    let _ = std::fs::write(&primary_path, &pretty);

    let downloads_dir = std::env::var("USERPROFILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("Downloads");
    let _ = std::fs::create_dir_all(&downloads_dir);
    let downloads_path = downloads_dir.join(&filename);
    let _ = std::fs::write(&downloads_path, &pretty);

    ExportResult {
        exported_at,
        export_path: primary_path.to_string_lossy().to_string(),
        download_path: downloads_path.to_string_lossy().to_string(),
        sessions: backup.sessions.len(),
        daily_stats: backup.daily_stats.len(),
        app_audio_events: backup.app_audio_events.len(),
        query_logs: backup.query_logs.len(),
    }
}

#[derive(serde::Serialize)]
struct ExportResult {
    exported_at: String,
    export_path: String,
    download_path: String,
    sessions: usize,
    daily_stats: usize,
    app_audio_events: usize,
    query_logs: usize,
}

#[tauri::command]
fn import_all_data(data: String) -> bool {
    match serde_json::from_str::<db::BackupData>(&data) {
        Ok(backup) => {
            db::import_backup(&backup);
            true
        }
        Err(_) => false,
    }
}

#[tauri::command]
fn show_notification(app: AppHandle, title: String, body: String) {
    #[cfg(target_os = "windows")]
    {
        use windows::core::HSTRING;
        use windows::Data::Xml::Dom::XmlDocument;
        use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

        let app_id = app.config().identifier.clone();
        let title = escape_xml_text(&title);
        let body = escape_xml_text(&body);
        std::thread::spawn(move || {
            let xml = format!(
                r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
                title,
                body
            );

            let Ok(doc) = XmlDocument::new() else { return; };
            if doc.LoadXml(&HSTRING::from(xml)).is_err() {
                return;
            }

            let Ok(toast) = ToastNotification::CreateToastNotification(&doc) else { return; };
            let Ok(notifier) = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id)) else {
                return;
            };

            let _ = notifier.Show(&toast);
        });
    }
}

fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[tauri::command]
fn get_paired_devices() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new("powershell");
        command.creation_flags(0x08000000);
        let output = command
            .args([
                "-NoProfile", "-NonInteractive", "-Command",
                "Get-PnpDevice -Class Bluetooth | Where-Object FriendlyName -notlike '*Enumerator*' | Where-Object FriendlyName -notlike '*Intel*' | Where-Object FriendlyName -notlike '*RFCOMM*' | Where-Object FriendlyName -notlike '*Microsoft*' | Where-Object FriendlyName -notlike '*Transport*' | Where-Object FriendlyName -notlike '*Adapter*' | Select-Object -ExpandProperty FriendlyName"
            ])
            .output();
        
        if let Ok(out) = output {
            let s = String::from_utf8_lossy(&out.stdout);
            let mut devices: Vec<String> = s.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            devices.sort();
            devices.dedup();
            devices
        } else {
            vec![]
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        vec![]
    }
}

#[tauri::command]
fn verify_windows_password(password: String) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::core::HSTRING;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::Security::{
            LogonUserW, LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT,
        };

        let username = std::env::var("USERNAME").unwrap_or_default();
        let mut token = windows::Win32::Foundation::HANDLE::default();

        let ok = unsafe {
            LogonUserW(
                &HSTRING::from(username.as_str()),
                &HSTRING::from("."), // "." = local machine
                &HSTRING::from(password.as_str()),
                LOGON32_LOGON_INTERACTIVE,
                LOGON32_PROVIDER_DEFAULT,
                &mut token,
            )
        };

        if ok.is_ok() {
            unsafe { let _ = CloseHandle(token); }
            true
        } else {
            false
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = password;
        false
    }
}

#[tauri::command]
fn get_device_battery(state: State<TrackerState>) -> Option<spp::BatteryInfo> {
    state.battery_cache.lock().clone()
}

#[tauri::command]
fn is_debug() -> bool {
    cfg!(debug_assertions)
}

// ── Startup (autostart) commands ─────────────────────────────────────────────

#[tauri::command]
fn get_startup_enabled(app: AppHandle) -> bool {
    // Prefer persisted user preference; fall back to the OS-registered state.
    let pref = read_startup_enabled_from_disk();
    if pref {
        return true;
    }
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_startup_enabled(enabled: bool, app: AppHandle) -> bool {
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    if result.is_ok() {
        write_startup_enabled_to_disk(enabled);
    }
    result.is_ok()
}

// ── Session Breakdown commands ──────────────────────────────────────────────

#[tauri::command]
fn get_sessions_for_breakdown() -> Vec<db::SessionBreakdownRow> {
    db::get_sessions_for_breakdown(200)
}

#[tauri::command]
fn get_session_breakdown(session_id: i64) -> Option<db::SessionBreakdown> {
    db::get_session_breakdown(session_id)
}

#[tauri::command]
fn set_session_note(session_id: i64, note: String) {
    db::set_session_note(session_id, &note);
}

#[tauri::command]
fn get_battery_graph_data(duration: String) -> Vec<db::BatteryGraphPoint> {
    db::get_battery_graph_data(&duration)
}

/// Returns a JSON or CSV string of the full session breakdown for client-side download.
#[tauri::command]
fn export_session(session_id: i64, format: String) -> String {
    let bd = match db::get_session_breakdown(session_id) {
        Some(b) => b,
        None => return String::new(),
    };

    if format.to_lowercase() == "csv" {
        // CSV: two sections – session summary, then app totals
        let mut out = String::new();
        out.push_str("id,session_start,session_end,connected_secs,playback_secs,");
        out.push_str("bat_left_connect,bat_right_connect,bat_case_connect,");
        out.push_str("bat_left_disc,bat_right_disc,bat_case_disc,notes\n");
        let s = &bd.session;
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{}\n",
            s.id, s.session_start, s.session_end,
            s.connected_secs, s.playback_secs,
            s.bat_left_connect.map(|v| v.to_string()).unwrap_or_default(),
            s.bat_right_connect.map(|v| v.to_string()).unwrap_or_default(),
            s.bat_case_connect.map(|v| v.to_string()).unwrap_or_default(),
            s.bat_left_disc.map(|v| v.to_string()).unwrap_or_default(),
            s.bat_right_disc.map(|v| v.to_string()).unwrap_or_default(),
            s.bat_case_disc.map(|v| v.to_string()).unwrap_or_default(),
            s.notes.as_deref().unwrap_or(""),
        ));
        out.push_str("\nprocess_name,total_secs,event_count\n");
        for t in &bd.app_totals {
            out.push_str(&format!("{},{},{}\n", t.process_name, t.total_secs, t.event_count));
        }
        out
    } else {
        // JSON
        serde_json::to_string_pretty(&bd).unwrap_or_default()
    }
}

// ── App entry ─────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    let mut initial_device = "CMF Buds 2a".to_string();
    let mut initial_interval = 300; // 5 minutes default
    let mut initial_step = 5; // 5% default
    if let Ok(appdata) = std::env::var("APPDATA") {
        let dir = std::path::PathBuf::from(appdata).join("EarbudsTracker");
        let path_dev = dir.join("target_device.txt");
        if let Ok(content) = std::fs::read_to_string(path_dev) {
            let trimmed = content.trim().to_string();
            if !trimmed.is_empty() {
                initial_device = trimmed;
            }
        }
        let path_int = dir.join("battery_interval.txt");
        if let Ok(content) = std::fs::read_to_string(path_int) {
            if let Ok(parsed) = content.trim().parse::<u64>() {
                initial_interval = parsed;
            }
        }
        let path_step = dir.join("battery_step.txt");
        if let Ok(content) = std::fs::read_to_string(path_step) {
            if let Ok(parsed) = content.trim().parse::<u8>() {
                if parsed == 1 || parsed == 5 || parsed == 10 {
                    initial_step = parsed;
                }
            }
        }
    }
    let tracker = Arc::new(Tracker::new(&initial_device, initial_interval, initial_step));

    // Honour persisted "Enable on Startup" preference and reflect it in the OS registry.
    let initial_startup_enabled = read_startup_enabled_from_disk();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--silent"]),
        ))
        .manage(Arc::clone(&tracker))
        .setup(move |app| {
            let handle = app.handle().clone();

            // Reflect the persisted "Enable on Startup" preference in the OS registry.
            // This keeps the registry state in sync with the user's last choice and
            // ensures the app appears in Task Manager → Startup Apps for installed builds.
            let auto_manager = handle.autolaunch();
            let currently_enabled = auto_manager.is_enabled().unwrap_or(false);
            if initial_startup_enabled && !currently_enabled {
                let _ = auto_manager.enable();
            } else if !initial_startup_enabled && currently_enabled {
                let _ = auto_manager.disable();
            }

            #[cfg(target_os = "windows")]
            {
                use windows::core::PCWSTR;
                use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

                let app_id = handle.config().identifier.clone();
                let mut app_id_wide: Vec<u16> = app_id.encode_utf16().collect();
                app_id_wide.push(0);
                unsafe {
                    let _ = SetCurrentProcessExplicitAppUserModelID(PCWSTR(app_id_wide.as_ptr()));
                }
            }

            // Wire state-change → push event to frontend
            let notify_handle = handle.clone();
            *tracker.on_state_change.lock() = Some(Box::new(move || {
                let _ = notify_handle.emit("state-changed", ());
            }));

            // Start background monitors
            tracker.start(handle.clone());

            // Check initial connection status: show window if earbuds are disconnected at launch
            let startup_handle = handle.clone();
            let startup_tracker = Arc::clone(&tracker);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                if !startup_tracker.is_connected() {
                    if let Some(win) = startup_handle.get_webview_window("main") {
                        win.show().ok();
                        win.set_focus().ok();
                    }
                }
            });

            // Single-instance: hide window on close, show on tray click
            #[cfg(desktop)]
            {
                let win = app.get_webview_window("main").unwrap();
                win.hide().ok();

                // Tray icon menu
                use tauri::menu::{MenuBuilder, MenuItemBuilder};
                let show_i = MenuItemBuilder::with_id("show", "Show Window").build(app)?;
                let quit_i = MenuItemBuilder::with_id("quit", "Exit").build(app)?;
                let menu = MenuBuilder::new(app).items(&[&show_i, &quit_i]).build()?;

                // Tray icon
                use tauri::tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState};
                let tray_handle = handle.clone();
                TrayIconBuilder::new()
                    .icon(app.default_window_icon().unwrap().clone())
                    .tooltip("Nox")
                    .menu(&menu)
                    .on_menu_event(move |app_handle, event| {
                        match event.id().as_ref() {
                            "quit" => {
                                app_handle.exit(0);
                            }
                            "show" => {
                                if let Some(w) = app_handle.get_webview_window("main") {
                                    w.show().ok();
                                    w.set_focus().ok();
                                }
                            }
                            _ => {}
                        }
                    })
                    .on_tray_icon_event(move |_tray, event| {
                        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                            if let Some(w) = tray_handle.get_webview_window("main") {
                                if w.is_visible().unwrap_or(false) {
                                    w.hide().ok();
                                } else {
                                    w.show().ok();
                                    w.set_focus().ok();
                                }
                            }
                        }
                    })
                    .build(app)?;
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                window.hide().ok();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot, get_sessions, reset_all, get_daily_history, get_daily_history_bounds,
            set_device_name, show_notification, get_paired_devices,
            verify_windows_password, get_device_battery, is_debug,
            get_sessions_for_breakdown, get_session_breakdown,
            set_session_note, export_session,
            get_battery_interval, set_battery_interval,
            get_battery_step, set_battery_step,
            get_battery_graph_data, get_active_audio_apps,
            get_query_log, export_all_data, import_all_data,
            get_startup_enabled, set_startup_enabled
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
