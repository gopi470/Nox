// lib.rs – Tauri command handlers + app setup
mod db;
mod bluetooth;
mod audio;
mod tracker;
mod spp;

use std::sync::Arc;
use tauri::{Manager, State, AppHandle, Emitter};
use tracker::{Tracker, Snapshot};
use db::SessionRow;

type TrackerState = Arc<Tracker>;

// ── Tauri commands (called from JS frontend) ──────────────────────────────────

#[tauri::command]
fn get_snapshot(state: State<TrackerState>) -> Snapshot {
    state.get_snapshot()
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
fn reset_all(state: State<TrackerState>) {
    state.reset_all();
}

#[tauri::command]
fn get_daily_history() -> Vec<db::DailyStatsRow> {
    db::get_daily_history(7)
}

#[tauri::command]
fn show_notification(title: String, body: String) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        std::thread::spawn(move || {
            let cmd = format!(
                "[void] [System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms'); \
                 $notification = New-Object System.Windows.Forms.NotifyIcon; \
                 $notification.Icon = [System.Drawing.SystemIcons]::Information; \
                 $notification.BalloonTipText = '{}'; \
                 $notification.BalloonTipTitle = '{}'; \
                 $notification.Visible = $true; \
                 $notification.ShowBalloonTip(5000);",
                body.replace("'", "''"),
                title.replace("'", "''")
            );
            let mut command = std::process::Command::new("powershell");
            command.creation_flags(0x08000000);
            let _ = command
                .args(["-NoProfile", "-NonInteractive", "-Command", &cmd])
                .output();
        });
    }
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
    #[cfg(debug_assertions)]
    {
        state.battery_cache.lock().clone()
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = state;
        None
    }
}

#[tauri::command]
fn is_debug() -> bool {
    cfg!(debug_assertions)
}

// ── App entry ─────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    let mut initial_device = "CMF Buds 2a".to_string();
    if let Ok(appdata) = std::env::var("APPDATA") {
        let path = std::path::PathBuf::from(appdata)
            .join("EarbudsTracker")
            .join("target_device.txt");
        if let Ok(content) = std::fs::read_to_string(path) {
            let trimmed = content.trim().to_string();
            if !trimmed.is_empty() {
                initial_device = trimmed;
            }
        }
    }
    let tracker = Arc::new(Tracker::new(&initial_device));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(Arc::clone(&tracker))
        .setup(move |app| {
            let handle = app.handle().clone();

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
                    .tooltip("EarbudsTracker")
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
        .invoke_handler(tauri::generate_handler![get_snapshot, get_sessions, reset_all, get_daily_history, set_device_name, show_notification, get_paired_devices, verify_windows_password, get_device_battery, is_debug])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
