// lib.rs – Tauri command handlers + app setup
mod db;
mod bluetooth;
mod audio;
mod app_audio;
mod tracker;
mod spp;

use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::{ManagerExt, MacosLauncher};
use serde::Deserialize;


use tracker::{Tracker, Snapshot};
use db::SessionRow;
use chrono::Local;

type TrackerState = Arc<Tracker>;

#[cfg(target_os = "windows")]
#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(
        hwnd: windows::Win32::Foundation::HWND,
        lpText: windows::core::PCWSTR,
        lpCaption: windows::core::PCWSTR,
        uType: u32,
    ) -> i32;
}


fn settings_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into()))
        .join("EarbudsTracker")
}

fn settings_file() -> std::path::PathBuf {
    settings_dir().join("settings.json")
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceProfile {
    pub friendly_name: String,
    pub brand: String,
    pub protocol_mode: String,
    pub battery_interval: u64,
    pub battery_step: u8,
    /// Bluetooth hardware MAC address stored as a hex string (e.g. "AABBCCDDEEFF").
    /// Saved when the user adds a profile from the paired-devices list.
    /// `None` for profiles created before this feature or migrated from legacy installs.
    #[serde(default)]
    pub mac_address: Option<String>,
}

// Unified application settings, persisted in `settings.json` under the OS app-data dir.
// Fields use `serde(default)` so older installs that are missing keys still load cleanly.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_battery_interval")]
    pub battery_interval: u64,
    #[serde(default = "default_battery_step")]
    pub battery_step: u8,
    #[serde(default = "default_target_device")]
    pub target_device: String,
    #[serde(default)]
    pub startup_enabled: bool,
    #[serde(default = "default_desktop_notifications")]
    pub desktop_notifications: bool,
    #[serde(default)]
    pub auto_backup_enabled: bool,
    #[serde(default = "default_auto_backup_interval")]
    pub auto_backup_interval: String,
    #[serde(default = "default_autopause_enabled")]
    pub autopause_enabled: bool,
    #[serde(default = "default_device_profiles")]
    pub device_profiles: Vec<DeviceProfile>,
    #[serde(default = "default_active_device")]
    pub active_device: String,
}

fn default_battery_interval() -> u64 { 300 }
fn default_battery_step() -> u8 { 5 }
fn default_target_device() -> String { "CMF Buds 2a".to_string() }
fn default_desktop_notifications() -> bool { true }
fn default_auto_backup_interval() -> String { "never".to_string() }
fn default_autopause_enabled() -> bool { true }
fn default_device_profiles() -> Vec<DeviceProfile> {
    vec![DeviceProfile {
        friendly_name: "CMF Buds 2a".to_string(),
        brand: "Nothing".to_string(),
        protocol_mode: "auto".to_string(),
        battery_interval: 300,
        battery_step: 5,
        mac_address: None,
    }]
}
fn default_active_device() -> String { "CMF Buds 2a".to_string() }

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            battery_interval: default_battery_interval(),
            battery_step: default_battery_step(),
            target_device: default_target_device(),
            startup_enabled: false,
            desktop_notifications: default_desktop_notifications(),
            auto_backup_enabled: false,
            auto_backup_interval: default_auto_backup_interval(),
            autopause_enabled: default_autopause_enabled(),
            device_profiles: default_device_profiles(),
            active_device: default_active_device(),
        }
    }
}

// Auto Backup interval identifiers. "never" disables the feature.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoBackupInterval {
    Never,
    OneDay,
    FiveDays,
    OneWeek,
    TwoWeeks,
    OneMonth,
}

impl AutoBackupInterval {
    fn as_key(self) -> &'static str {
        match self {
            AutoBackupInterval::Never => "never",
            AutoBackupInterval::OneDay => "1_day",
            AutoBackupInterval::FiveDays => "5_days",
            AutoBackupInterval::OneWeek => "1_week",
            AutoBackupInterval::TwoWeeks => "2_weeks",
            AutoBackupInterval::OneMonth => "1_month",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "1_day" => AutoBackupInterval::OneDay,
            "5_days" => AutoBackupInterval::FiveDays,
            "1_week" => AutoBackupInterval::OneWeek,
            "2_weeks" => AutoBackupInterval::TwoWeeks,
            "1_month" => AutoBackupInterval::OneMonth,
            _ => AutoBackupInterval::Never,
        }
    }

    fn duration_secs(self) -> Option<u64> {
        match self {
            AutoBackupInterval::Never => None,
            AutoBackupInterval::OneDay => Some(24 * 60 * 60),
            AutoBackupInterval::FiveDays => Some(5 * 24 * 60 * 60),
            AutoBackupInterval::OneWeek => Some(7 * 24 * 60 * 60),
            AutoBackupInterval::TwoWeeks => Some(14 * 24 * 60 * 60),
            AutoBackupInterval::OneMonth => Some(30 * 24 * 60 * 60),
        }
    }
}

pub(crate) fn load_settings() -> AppSettings {
    let path = settings_file();
    let mut settings = match std::fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<AppSettings>(&raw) {
            Ok(s) => s,
            Err(err) => {
                log::warn!("settings.json could not be parsed ({}); falling back to defaults", err);
                AppSettings::default()
            }
        },
        Err(_) => AppSettings::default(),
    };

    if settings.device_profiles.is_empty() {
        let name = settings.target_device.clone();
        let brand = if name.to_uppercase().contains("CMF") || name.to_uppercase().contains("NOTHING") {
            "nothing_cmf".to_string()
        } else {
            "generic_other".to_string()
        };
        settings.device_profiles = vec![DeviceProfile {
            friendly_name: name.clone(),
            brand,
            protocol_mode: "auto".to_string(),
            battery_interval: settings.battery_interval,
            battery_step: settings.battery_step,
            mac_address: None,
        }];
        settings.active_device = name;
    }
    settings
}

fn save_settings_to_disk(settings: &AppSettings) {
    let dir = settings_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(pretty) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(settings_file(), pretty);
    }
}

fn update_settings<F: FnOnce(&mut AppSettings)>(mutator: F) -> AppSettings {
    let mut current = load_settings();
    mutator(&mut current);
    save_settings_to_disk(&current);
    current
}

// Backwards-compatible wrappers that read/write the single `startup_enabled`
// field inside `settings.json` without changing the existing call sites.
fn read_startup_enabled_from_disk() -> bool {
    load_settings().startup_enabled
}

fn write_startup_enabled_to_disk(enabled: bool) {
    update_settings(|s| s.startup_enabled = enabled);
}

// ── Auto Backup scheduler helpers ──────────────────────────────────────────────
//
// The scheduler stores the timestamp of the last successful auto backup in a
// small sibling file (`auto_backup_state.json`) so the interval countdown
// survives app restarts without polluting the main `AppSettings` struct.

fn auto_backup_state_file() -> std::path::PathBuf {
    settings_dir().join("auto_backup_state.json")
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct AutoBackupState {
    #[serde(default)]
    last_run_at: Option<String>,
}

fn load_auto_backup_state() -> AutoBackupState {
    match std::fs::read_to_string(auto_backup_state_file()) {
        Ok(raw) => serde_json::from_str::<AutoBackupState>(&raw).unwrap_or_default(),
        Err(_) => AutoBackupState::default(),
    }
}

fn save_auto_backup_state(state: &AutoBackupState) {
    let _ = std::fs::create_dir_all(settings_dir());
    if let Ok(pretty) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(auto_backup_state_file(), pretty);
    }
}

fn parse_rfc3339_to_epoch(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.timestamp())
}

fn auto_backup_due(enabled: bool, interval_key: &str, last_run: Option<&str>) -> bool {
    if !enabled {
        return false;
    }
    let secs = AutoBackupInterval::parse(interval_key).duration_secs();
    let secs = match secs {
        Some(s) => s,
        None => return false, // "never" or unknown
    };
    let last = last_run.and_then(parse_rfc3339_to_epoch).unwrap_or(0);
    let now = Local::now().timestamp();
    now - last >= secs as i64
}

// Run an auto backup and return the AutoBackup folder path (for scheduler logging).
fn run_auto_backup_now_opt_path() -> Option<String> {
    let result = run_auto_backup_now();
    Some(result.auto_backup_path)
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
    update_settings(|s| s.target_device = name);
}

#[tauri::command]
fn get_battery_interval(state: State<TrackerState>) -> u64 {
    state.get_battery_interval_secs()
}

#[tauri::command]
fn set_battery_interval(secs: u64, state: State<TrackerState>) {
    state.set_battery_interval_secs(secs);
    update_settings(|s| s.battery_interval = secs);
}

#[tauri::command]
fn get_battery_step(state: State<TrackerState>) -> u8 {
    state.get_battery_step()
}

#[tauri::command]
fn set_battery_step(step: u8, state: State<TrackerState>) {
    state.set_battery_step(step);
    update_settings(|s| s.battery_step = step);
}

#[tauri::command]
fn reset_all(state: State<TrackerState>) {
    state.reset_all();
}

#[tauri::command]
fn get_daily_history(week_offset: i64, state: State<TrackerState>) -> Vec<db::DailyStatsRow> {
    db::get_daily_history(&state.get_device_name(), week_offset)
}

#[tauri::command]
fn get_daily_history_bounds(state: State<TrackerState>) -> db::DailyHistoryBounds {
    db::get_daily_history_bounds(&state.get_device_name())
}

#[tauri::command]
fn get_query_log(state: State<TrackerState>) -> Vec<db::QueryLogRow> {
    state.get_query_log()
}

// Build a JSON backup payload from the current database state. Shared by
// manual exports (export_all_data) and the auto-backup scheduler.
fn build_backup_json() -> (String, usize, usize, usize, usize, String) {
    let backup = db::export_backup();
    let exported_at = Local::now().to_rfc3339();
    let pretty = serde_json::to_string_pretty(&backup).unwrap_or_default();
    (
        pretty,
        backup.sessions.len(),
        backup.daily_stats.len(),
        backup.app_audio_events.len(),
        backup.query_logs.len(),
        exported_at,
    )
}

// Build the timestamped backup filename (e.g. "backup_2026-06-08_14-30-00.json").
fn backup_filename() -> String {
    format!(
        "backup_{}.json",
        Local::now().format("%Y-%m-%d_%H-%M-%S")
    )
}

fn write_backup_to_dir(dir: &std::path::Path, filename: &str, pretty: &str) -> std::path::PathBuf {
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join(filename);
    let _ = std::fs::write(&path, pretty);
    path
}

fn downloads_dir() -> std::path::PathBuf {
    std::env::var("USERPROFILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("Downloads")
}

#[tauri::command]
fn export_all_data() -> ExportResult {
    let (pretty, sessions, daily_stats, app_audio_events, query_logs, exported_at) =
        build_backup_json();
    let filename = format!(
        "nox-backup-{}.json",
        Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    let primary_dir = std::path::PathBuf::from(appdata)
        .join("EarbudsTracker")
        .join("exports");
    let primary_path = write_backup_to_dir(&primary_dir, &filename, &pretty);

    let downloads_path = write_backup_to_dir(&downloads_dir(), &filename, &pretty);

    ExportResult {
        exported_at,
        export_path: primary_path.to_string_lossy().to_string(),
        download_path: downloads_path.to_string_lossy().to_string(),
        sessions,
        daily_stats,
        app_audio_events,
        query_logs,
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

// Runs an auto-backup write to both target folders and returns a summary.
// Used by the scheduler thread and exposed to the frontend for manual triggers.
fn run_auto_backup_now() -> AutoBackupResult {
    let (pretty, sessions, daily_stats, app_audio_events, query_logs, exported_at) =
        build_backup_json();
    let filename = backup_filename();

    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    let auto_dir = std::path::PathBuf::from(appdata)
        .join("EarbudsTracker")
        .join("AutoBackup");
    let auto_path = write_backup_to_dir(&auto_dir, &filename, &pretty);
    let downloads_path = write_backup_to_dir(&downloads_dir(), &filename, &pretty);

    AutoBackupResult {
        exported_at,
        auto_backup_path: auto_path.to_string_lossy().to_string(),
        download_path: downloads_path.to_string_lossy().to_string(),
        sessions,
        daily_stats,
        app_audio_events,
        query_logs,
    }
}

#[derive(serde::Serialize)]
struct AutoBackupResult {
    exported_at: String,
    auto_backup_path: String,
    download_path: String,
    sessions: usize,
    daily_stats: usize,
    app_audio_events: usize,
    query_logs: usize,
}

#[tauri::command]
fn import_all_data(data: String, state: State<TrackerState>) -> bool {
    match serde_json::from_str::<db::BackupData>(&data) {
        Ok(backup) => {
            db::import_backup(&backup, &state.get_device_name());
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

            if notifier.Show(&toast).is_ok() {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let _ = notifier.Hide(&toast);
            }
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PairedDevice {
    pub name: String,
    pub mac: String, // 12-char hex, e.g. "001A7DDA7111"
}

#[tauri::command]
async fn get_paired_devices() -> Vec<PairedDevice> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // Output "FriendlyName|MAC" pairs so we capture both in one pass.
        // The InstanceId for Bluetooth PnP devices contains DEV_<12-hex-char-MAC>.
        let script = r#"
Get-PnpDevice -Class Bluetooth |
  Where-Object { $_.FriendlyName -notlike '*Enumerator*' -and
                 $_.FriendlyName -notlike '*Intel*'      -and
                 $_.FriendlyName -notlike '*RFCOMM*'     -and
                 $_.FriendlyName -notlike '*Microsoft*'  -and
                 $_.FriendlyName -notlike '*Transport*'  -and
                 $_.FriendlyName -notlike '*Adapter*' } |
  ForEach-Object {
    $mac = ''
    if ($_.InstanceId -match 'DEV_([0-9A-Fa-f]{12})') { $mac = $Matches[1] }
    "$($_.FriendlyName)|$mac"
  }
"#;
        let mut command = std::process::Command::new("powershell");
        command.creation_flags(0x08000000);
        let output = command
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output();

        if let Ok(out) = output {
            let s = String::from_utf8_lossy(&out.stdout);
            let mut seen = std::collections::HashSet::new();
            let mut devices: Vec<PairedDevice> = s
                .lines()
                .filter_map(|l| {
                    let l = l.trim();
                    if l.is_empty() { return None; }
                    let mut parts = l.splitn(2, '|');
                    let name = parts.next()?.trim().to_string();
                    let mac  = parts.next().unwrap_or("").trim().to_string();
                    if name.is_empty() { return None; }
                    Some(PairedDevice { name, mac })
                })
                .filter(|d| seen.insert(d.name.clone()))
                .collect();
            devices.sort_by(|a, b| a.name.cmp(&b.name));
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
async fn force_query_battery(state: tauri::State<'_, TrackerState>) -> Result<Option<spp::BatteryInfo>, String> {
    Ok(state.force_query_battery())
}

#[tauri::command]
fn is_debug() -> bool {
    cfg!(debug_assertions)
}

#[tauri::command]
fn get_autopause_enabled() -> bool {
    load_settings().autopause_enabled
}

#[tauri::command]
fn set_autopause_enabled(enabled: bool) {
    update_settings(|s| s.autopause_enabled = enabled);
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

// ── App metadata ──────────────────────────────────────────────────────────────

// Cached app version parsed from `tauri.conf.json` at startup so the JS
// frontend can read it with a single cheap `get_app_version` invoke.
use std::sync::OnceLock;
static APP_VERSION: OnceLock<String> = OnceLock::new();

fn init_app_version_from_tauri_config() {
    // Prefer the `tauri.conf.json` version string (which is also what the
    // bundle metadata uses). Fall back to `CARGO_PKG_VERSION` if the config
    // file can't be parsed, so the UI never shows a blank version.
    let from_config = std::fs::read_to_string("tauri.conf.json")
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| {
            v.get("version")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        });
    let version = from_config.unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let _ = APP_VERSION.set(version);
}

#[tauri::command]
fn get_device_profiles() -> (String, Vec<DeviceProfile>) {
    let s = load_settings();
    (s.active_device, s.device_profiles)
}

#[tauri::command]
fn switch_active_profile(name: String, state: tauri::State<'_, TrackerState>) -> Result<DeviceProfile, String> {
    let mut updated_profile: Option<DeviceProfile> = None;
    let _s = update_settings(|s| {
        if s.device_profiles.iter().any(|p| p.friendly_name == name) {
            s.active_device = name.clone();
            s.target_device = name.clone();
            if let Some(p) = s.device_profiles.iter().find(|p| p.friendly_name == name) {
                s.battery_interval = p.battery_interval;
                s.battery_step = p.battery_step;
                updated_profile = Some(p.clone());
            }
        }
    });

    if let Some(profile) = updated_profile {
        // Reconfigure the running tracker instance
        state.set_device_name(&profile.friendly_name);
        state.set_mac_address(profile.mac_address.clone());
        state.set_brand(&profile.brand);
        state.set_protocol_mode(&profile.protocol_mode);
        state.set_battery_interval_secs(profile.battery_interval);
        state.set_battery_step(profile.battery_step);
        Ok(profile)
    } else {
        Err("Profile not found".to_string())
    }
}

#[tauri::command]
fn save_device_profile(profile: DeviceProfile, state: tauri::State<'_, TrackerState>) -> Result<(), String> {
    let name = profile.friendly_name.clone();
    let mut is_active = false;
    let _s = update_settings(|s| {
        if let Some(idx) = s.device_profiles.iter().position(|p| p.friendly_name == name) {
            s.device_profiles[idx] = profile.clone();
        } else {
            s.device_profiles.push(profile.clone());
        }
        if s.active_device == name {
            s.target_device = name.clone();
            s.battery_interval = profile.battery_interval;
            s.battery_step = profile.battery_step;
            is_active = true;
        }
    });

    if is_active {
        state.set_device_name(&profile.friendly_name);
        state.set_mac_address(profile.mac_address.clone());
        state.set_brand(&profile.brand);
        state.set_protocol_mode(&profile.protocol_mode);
        state.set_battery_interval_secs(profile.battery_interval);
        state.set_battery_step(profile.battery_step);
    }
    Ok(())
}

/// Creates a brand-new profile. If a profile with the same `friendly_name` already exists
/// (e.g. two physical "CMF Buds 2a" devices), the name is auto-suffixed to "(2)", "(3)", etc.
/// Returns the actual name under which the profile was saved.
#[tauri::command]
fn create_device_profile(profile: DeviceProfile, _state: tauri::State<'_, TrackerState>) -> Result<String, String> {
    let base_name = profile.friendly_name.trim().to_string();
    let mut final_name = String::new();

    let _s = update_settings(|s| {
        // Collect all existing profile names
        let existing_names: std::collections::HashSet<&str> =
            s.device_profiles.iter().map(|p| p.friendly_name.as_str()).collect();

        // Find an unused name: try base_name, then base_name (2), (3), ...
        let candidate = if !existing_names.contains(base_name.as_str()) {
            base_name.clone()
        } else {
            let mut n = 2u32;
            loop {
                let candidate = format!("{} ({})", base_name, n);
                if !existing_names.contains(candidate.as_str()) {
                    break candidate;
                }
                n += 1;
            }
        };

        final_name = candidate.clone();
        let mut new_profile = profile.clone();
        new_profile.friendly_name = candidate;
        s.device_profiles.push(new_profile);
    });

    Ok(final_name)
}

#[tauri::command]
fn delete_device_profile(name: String, state: tauri::State<'_, TrackerState>) -> Result<(), String> {
    let mut fallback_profile: Option<DeviceProfile> = None;
    let mut active_was_deleted = false;

    let _s = update_settings(|s| {
        s.device_profiles.retain(|p| p.friendly_name != name);
        if s.active_device == name {
            active_was_deleted = true;
            if let Some(first_p) = s.device_profiles.first() {
                s.active_device = first_p.friendly_name.clone();
                s.target_device = first_p.friendly_name.clone();
                s.battery_interval = first_p.battery_interval;
                s.battery_step = first_p.battery_step;
                fallback_profile = Some(first_p.clone());
            } else {
                s.active_device = String::new();
                s.target_device = String::new();
                s.battery_interval = 300; // default 5m
                s.battery_step = 5;       // default 5%
            }
        }
    });

    db::delete_profile_data(&name);

    if active_was_deleted {
        if let Some(profile) = fallback_profile {
            state.set_device_name(&profile.friendly_name);
            state.set_battery_interval_secs(profile.battery_interval);
            state.set_battery_step(profile.battery_step);
        } else {
            state.set_device_name("");
            state.set_battery_interval_secs(300);
            state.set_battery_step(5);
        }
    }

    Ok(())
}

#[tauri::command]
fn is_bluetooth_enabled() -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::Devices::Radios::{Radio, RadioKind, RadioState};
        let Ok(radios) = Radio::GetRadiosAsync().and_then(|op| op.get()) else {
            return false;
        };
        for radio in radios {
            if let Ok(kind) = radio.Kind() {
                if kind == RadioKind::Bluetooth {
                    if let Ok(state) = radio.State() {
                        return state == RadioState::On;
                    }
                }
            }
        }
        false
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

#[tauri::command]
fn get_app_version() -> String {
    "1.0.0".to_string()
}

#[tauri::command]
fn open_url(url: String) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("cmd");
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        cmd.args(["/C", "start", "", &url]).spawn().ok();
    }
}

// ── Auto Backup commands ──────────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct AutoBackupSettings {
    enabled: bool,
    interval: String,
}

#[tauri::command]
fn get_auto_backup_settings() -> AutoBackupSettings {
    let s = load_settings();
    AutoBackupSettings {
        enabled: s.auto_backup_enabled,
        interval: s.auto_backup_interval,
    }
}

#[tauri::command]
fn set_auto_backup_settings(interval: String) -> AutoBackupSettings {
    // The dropdown IS the toggle: any non-"never" interval turns auto backup on.
    // Normalise the interval into a known key so unknown strings fall back to "never".
    let normalised = AutoBackupInterval::parse(&interval).as_key().to_string();
    let enabled = normalised != "never";
    let updated = update_settings(|s| {
        s.auto_backup_enabled = enabled;
        s.auto_backup_interval = normalised.clone();
    });
    AutoBackupSettings {
        enabled: updated.auto_backup_enabled,
        interval: updated.auto_backup_interval,
    }
}

#[tauri::command]
fn run_auto_backup() -> AutoBackupResult {
    let result = run_auto_backup_now();
    // Record the last successful run so the scheduler can compute the next due time.
    save_auto_backup_state(&AutoBackupState {
        last_run_at: Some(result.exported_at.clone()),
    });
    result
}

// ── Session Breakdown commands ──────────────────────────────────────────────

#[tauri::command]
fn get_sessions_for_breakdown(state: State<TrackerState>) -> Vec<db::SessionBreakdownRow> {
    db::get_sessions_for_breakdown(Some(&state.get_device_name()), 200)
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
fn get_battery_graph_data(duration: String, state: State<TrackerState>) -> Vec<db::BatteryGraphPoint> {
    db::get_battery_graph_data(&state.get_device_name(), &duration)
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

static IS_EXITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    // Pre-load the application version from `tauri.conf.json` so the
    // frontend can fetch it cheaply via the `get_app_version` command
    // and the About section always reflects the source of truth.
    init_app_version_from_tauri_config();

    // Load unified settings. If `settings.json` is missing (older installs), fall back
    // to the legacy per-setting text files and persist a fresh `settings.json` so
    // subsequent launches use the unified storage.
    let mut initial_device = "CMF Buds 2a".to_string();
    let mut initial_interval = 300; // 5 minutes default
    let mut initial_step = 5; // 5% default

    let mut settings = load_settings();
    let mut migrated_from_legacy = false;

    if let Ok(appdata) = std::env::var("APPDATA") {
        let dir = std::path::PathBuf::from(appdata).join("EarbudsTracker");

        // If settings.json was missing/empty, attempt to migrate from the legacy files.
        if !settings_file().exists() {
            let path_dev = dir.join("target_device.txt");
            if let Ok(content) = std::fs::read_to_string(&path_dev) {
                let trimmed = content.trim().to_string();
                if !trimmed.is_empty() {
                    settings.target_device = trimmed;
                    migrated_from_legacy = true;
                }
            }
            let path_int = dir.join("battery_interval.txt");
            if let Ok(content) = std::fs::read_to_string(&path_int) {
                if let Ok(parsed) = content.trim().parse::<u64>() {
                    settings.battery_interval = parsed;
                    migrated_from_legacy = true;
                }
            }
            let path_step = dir.join("battery_step.txt");
            if let Ok(content) = std::fs::read_to_string(&path_step) {
                if let Ok(parsed) = content.trim().parse::<u8>() {
                    if parsed == 1 || parsed == 5 || parsed == 10 {
                        settings.battery_step = parsed;
                        migrated_from_legacy = true;
                    }
                }
            }
        }
    }

    if !settings.target_device.trim().is_empty() {
        initial_device = settings.target_device.clone();
    }
    initial_interval = settings.battery_interval;
    if settings.battery_step == 1 || settings.battery_step == 5 || settings.battery_step == 10 {
        initial_step = settings.battery_step;
    }

    if migrated_from_legacy {
        save_settings_to_disk(&settings);
    }

    let initial_profile = settings
        .device_profiles
        .iter()
        .find(|p| p.friendly_name == initial_device);
    let initial_mac = initial_profile.and_then(|p| p.mac_address.clone());
    let initial_brand = initial_profile
        .map(|p| p.brand.clone())
        .unwrap_or_else(|| "generic_other".to_string());
    let initial_proto = initial_profile
        .map(|p| p.protocol_mode.clone())
        .unwrap_or_else(|| "auto".to_string());

    let tracker = Arc::new(Tracker::new(&initial_device, initial_mac, &initial_brand, &initial_proto, initial_interval, initial_step));

    // Honour persisted "Enable on Startup" preference and reflect it in the OS registry.
    let initial_startup_enabled = read_startup_enabled_from_disk();

    // Snapshot of the persisted Auto Backup preferences for the scheduler thread.
    let initial_auto_backup = settings.auto_backup_enabled;
    let initial_auto_backup_interval = settings.auto_backup_interval.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--silent"]),
        ))
        .manage(Arc::clone(&tracker))
        .setup(move |app| {
            IS_EXITING.store(false, std::sync::atomic::Ordering::Relaxed);
            let handle = app.handle().clone();

            // Reflect the persisted "Enable on Startup" preference in the OS registry.
            // This keeps the registry state in sync with the user's last choice and
            // ensures the app appears in Task Manager → Startup Apps for installed builds.
            let auto_manager = handle.autolaunch();
            let currently_enabled = auto_manager.is_enabled().unwrap_or(false);
            if initial_startup_enabled && !currently_enabled {
                let _ = auto_manager.enable();
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

            // Wire protocol discovery → persist the discovered mode back into settings.json
            // so subsequent connections skip the SPP probe and go straight to the right method.
            {
                let tracker_ref = Arc::clone(&tracker);
                *tracker.on_protocol_discovered.lock() = Some(Box::new(move |discovered: String| {
                    let dev = tracker_ref.get_device_name();
                    if dev.is_empty() { return; }
                    update_settings(|s| {
                        if let Some(p) = s.device_profiles.iter_mut().find(|p| p.friendly_name == dev) {
                            if p.protocol_mode == "auto" {
                                log::info!("Protocol discovered for '{}': {}", dev, discovered);
                                p.protocol_mode = discovered.clone();
                            }
                        }
                    });
                }));
            }

            // Start background monitors
            tracker.start(handle.clone());

            // Auto Backup scheduler: ticks every hour and writes a backup when
            // the persisted interval has elapsed. Uses the same shared
            // helpers as manual exports (run_auto_backup_now), so the JSON
            // format and the AutoBackup/Downloads destinations stay in sync.
            if initial_auto_backup {
                let scheduler_interval = initial_auto_backup_interval.clone();
                std::thread::spawn(move || {
                    // On startup, if a backup is overdue, run it immediately.
                    let last_state = load_auto_backup_state();
                    if auto_backup_due(
                        true,
                        &scheduler_interval,
                        last_state.last_run_at.as_deref(),
                    ) {
                        if let Some(path) = run_auto_backup_now_opt_path() {
                            save_auto_backup_state(&AutoBackupState {
                                last_run_at: Some(Local::now().to_rfc3339()),
                            });
                            log::info!("auto backup on startup → {}", path);
                        }
                    }

                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(60 * 60));
                        let current = load_settings();
                        if !current.auto_backup_enabled {
                            continue;
                        }
                        let last = load_auto_backup_state();
                        if auto_backup_due(
                            true,
                            &current.auto_backup_interval,
                            last.last_run_at.as_deref(),
                        ) {
                            if let Some(path) = run_auto_backup_now_opt_path() {
                                save_auto_backup_state(&AutoBackupState {
                                    last_run_at: Some(Local::now().to_rfc3339()),
                                });
                                log::info!("auto backup scheduled → {}", path);
                            }
                        }
                    }
                });
            }

            // Check initial connection status: show window if earbuds are disconnected at launch
            let startup_handle = handle.clone();
            let startup_tracker = Arc::clone(&tracker);
            let is_silent = std::env::args().any(|arg| arg == "--silent");
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                if !is_silent && !startup_tracker.is_connected() {
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
                let tray_tracker = Arc::clone(&tracker);
                TrayIconBuilder::new()
                    .icon(app.default_window_icon().unwrap().clone())
                    .tooltip("Nox")
                    .menu(&menu)
                    .on_menu_event(move |app_handle, event| {
                        match event.id().as_ref() {
                            "quit" => {
                                if tray_tracker.is_connected() {
                                    #[cfg(target_os = "windows")]
                                    {
                                        use windows::core::{HSTRING, PCWSTR};
                                        use windows::Win32::Foundation::HWND;

                                        let text = HSTRING::from("A live tracking session is currently active. Are you sure you want to exit Nox?");
                                        let caption = HSTRING::from("Exit Nox");
                                        let confirm = unsafe {
                                            MessageBoxW(
                                                HWND::default(),
                                                PCWSTR(text.as_wide().as_ptr()),
                                                PCWSTR(caption.as_wide().as_ptr()),
                                                0x00000004 | 0x00000030 | 0x00040000,
                                            )
                                        };
                                        if confirm != 6 {
                                            return;
                                        }
                                    }
                                }
                                IS_EXITING.store(true, std::sync::atomic::Ordering::Relaxed);
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
                if !IS_EXITING.load(std::sync::atomic::Ordering::Relaxed) {
                    window.hide().ok();
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot, get_sessions, reset_all, get_daily_history, get_daily_history_bounds,
            set_device_name, show_notification, get_paired_devices,
            verify_windows_password, get_device_battery, force_query_battery, is_debug,
            get_sessions_for_breakdown, get_session_breakdown,
            set_session_note, export_session,
            get_battery_interval, set_battery_interval,
            get_battery_step, set_battery_step,
            get_battery_graph_data, get_active_audio_apps,
            get_query_log, export_all_data, import_all_data,
            get_startup_enabled, set_startup_enabled,
            get_app_version, open_url,
            get_auto_backup_settings, set_auto_backup_settings, run_auto_backup,
            get_autopause_enabled, set_autopause_enabled,
            is_bluetooth_enabled,
            get_device_profiles, switch_active_profile,
            save_device_profile, delete_device_profile, create_device_profile
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
