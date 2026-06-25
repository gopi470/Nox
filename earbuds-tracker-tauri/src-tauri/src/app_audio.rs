// app_audio.rs – Per-app audio session tracker
//
// Polls IAudioSessionManager2 every second on the target audio device.
// Detects which processes are actively outputting audio (peak > threshold)
// and fires on_start / on_stop callbacks with (db_event_id, process_name).
//
// The caller (tracker.rs) writes those events to the DB and stores the
// returned event_id so on_stop can close the correct row.

use log::{debug, info};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const SILENCE_THRESHOLD: f32 = 0.001;
const GRACE_CHECKS: u32 = 8;

pub struct AppAudioMonitor {
    device_name: Arc<RwLock<String>>,
    stop_flag: Arc<AtomicBool>,
}

impl AppAudioMonitor {
    pub fn new(device_name: Arc<RwLock<String>>) -> Self {
        Self {
            device_name,
            stop_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Spawn the background polling thread.
    ///
    /// * `get_session_id` – returns the current active DB session id (None if idle)
    /// * `on_start(session_id, process_name) -> db_event_id`
    /// * `on_stop(db_event_id, process_name)`
    pub fn start(
        &self,
        get_session_id: impl Fn() -> Option<i64> + Send + 'static,
        on_start: impl Fn(i64, String) -> i64 + Send + 'static,
        on_stop: impl Fn(i64, String) + Send + 'static,
    ) {
        let device_name_lock = Arc::clone(&self.device_name);
        let stop_flag = Arc::clone(&self.stop_flag);

        std::thread::Builder::new()
            .name("AppAudioMonitor".into())
            .spawn(move || {
                #[cfg(target_os = "windows")]
                unsafe {
                    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                }

                // key: process_name  value: (db_event_id, silence_ticks)
                let mut active: HashMap<String, (i64, u32)> = HashMap::new();

                while !stop_flag.load(Ordering::SeqCst) {
                    let dev_name = device_name_lock.read().to_lowercase();
                    let sess_id = get_session_id();

                    if let Some(sid) = sess_id {
                        let playing_now = get_active_app_peaks(&dev_name);

                        // Newly active apps
                        for app in &playing_now {
                            if let Some(entry) = active.get_mut(app) {
                                entry.1 = 0; // reset silence counter
                            } else {
                                info!("AppAudio START: {app} (session {sid})");
                                let eid = on_start(sid, app.clone());
                                active.insert(app.clone(), (eid, 0));
                            }
                        }

                        // Silent apps – increment grace counter
                        let mut to_close: Vec<(String, i64)> = Vec::new();
                        for (app, (eid, silence)) in active.iter_mut() {
                            if !playing_now.contains(app) {
                                *silence += 1;
                                if *silence >= GRACE_CHECKS {
                                    info!("AppAudio STOP: {app} (event {eid})");
                                    to_close.push((app.clone(), *eid));
                                }
                            }
                        }
                        for (app, eid) in to_close {
                            active.remove(&app);
                            on_stop(eid, app);
                        }
                    } else {
                        // Session ended – close all open app events
                        let drained: Vec<(String, i64)> = active
                            .drain()
                            .map(|(app, (eid, _))| (app, eid))
                            .collect();
                        for (app, eid) in drained {
                            on_stop(eid, app);
                        }
                    }

                    std::thread::sleep(POLL_INTERVAL);
                }

                // Final cleanup
                for (app, (eid, _)) in active.drain() {
                    on_stop(eid, app);
                }
            })
            .expect("Failed to spawn AppAudioMonitor thread");
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }
}

// ── Windows WASAPI implementation ──────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub fn get_active_app_peaks(target_device: &str) -> Vec<String> {
    get_active_app_peaks_internal(Some(target_device))
}

#[cfg(target_os = "windows")]
fn get_active_app_peaks_internal(target_device: Option<&str>) -> Vec<String> {
    use windows::{
        core::Interface,
        Win32::{
            Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
            Foundation::CloseHandle,
            Media::Audio::{
                eRender, IAudioSessionControl, IAudioSessionControl2,
                IAudioSessionManager2, IMMDeviceCollection, IMMDeviceEnumerator,
                MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
                Endpoints::IAudioMeterInformation,
            },
            System::Com::{
                CoCreateInstance, CoTaskMemFree, StructuredStorage::PropVariantToStringAlloc,
                CLSCTX_ALL, STGM_READ,
            },
            System::Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    };

    let mut result: Vec<String> = Vec::new();

    unsafe {
        let enumerator: IMMDeviceEnumerator =
            match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                Ok(e) => e,
                Err(_) => return result,
            };

        let collection: IMMDeviceCollection =
            match enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) {
                Ok(c) => c,
                Err(_) => return result,
            };

        let count = match collection.GetCount() {
            Ok(n) => n,
            Err(_) => return result,
        };

        for i in 0..count {
            let device = match collection.Item(i) {
                Ok(d) => d,
                Err(_) => continue,
            };

            // Filter to target device by friendly name
            let store = match device.OpenPropertyStore(STGM_READ) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let prop = match store.GetValue(&PKEY_Device_FriendlyName) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let psz = match PropVariantToStringAlloc(&prop) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let friendly = psz.to_string().unwrap_or_default().to_lowercase();
            CoTaskMemFree(Some(psz.0 as *const _));

            if let Some(tgt) = target_device {
                if !friendly.contains(tgt) {
                    continue;
                }
            }

            // Activate IAudioSessionManager2 on this device
            let manager: IAudioSessionManager2 = match device.Activate(CLSCTX_ALL, None) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let session_enum = match manager.GetSessionEnumerator() {
                Ok(e) => e,
                Err(_) => continue,
            };

            let sess_count = match session_enum.GetCount() {
                Ok(n) => n,
                Err(_) => continue,
            };

            for j in 0..sess_count {
                let ctrl: IAudioSessionControl = match session_enum.GetSession(j) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                // Get PID
                let ctrl2: IAudioSessionControl2 = match ctrl.cast() {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let pid = match ctrl2.GetProcessId() {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                if pid == 0 {
                    continue; // System audio session
                }

                // Check peak level
                let meter: IAudioMeterInformation = match ctrl.cast() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let peak = match meter.GetPeakValue() {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                if peak < SILENCE_THRESHOLD {
                    continue;
                }

                // Resolve process name from PID
                let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    Ok(h) => h,
                    Err(_) => continue,
                };
                let mut buf = vec![0u16; 1024];
                let mut len = buf.len() as u32;
                if QueryFullProcessImageNameW(
                    handle,
                    PROCESS_NAME_WIN32,
                    windows::core::PWSTR(buf.as_mut_ptr()),
                    &mut len,
                )
                .is_ok()
                {
                    let full_path = String::from_utf16_lossy(&buf[..len as usize]);
                    let stem = std::path::Path::new(&full_path)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Unknown")
                        .to_string();
                    debug!("AppAudio active: {stem} peak={peak:.4}");
                    if !result.contains(&stem) {
                        result.push(stem);
                    }
                }
                let _ = CloseHandle(handle);
            }
        }
    }

    result
}

// ── All-endpoints variant (used by Dashboard Now Playing) ────────────────────
// Same as get_active_app_peaks but scans ALL active render endpoints, not just
// the one matching the target device name. This is more reliable for the UI
// because Windows may name the Bluetooth endpoint differently at the WASAPI level.

#[cfg(target_os = "windows")]
pub fn get_all_app_peaks() -> Vec<String> {
    // Reuse the same logic but pass an empty target so the device filter always matches
    get_active_app_peaks_internal(None)
}

#[cfg(target_os = "windows")]
pub fn get_active_capture_processes() -> Vec<String> {
    use windows::{
        core::Interface,
        Win32::{
            Media::Audio::{
                eCapture, IAudioSessionControl, IAudioSessionControl2,
                IAudioSessionManager2, IMMDeviceCollection, IMMDeviceEnumerator,
                MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
            },
            System::Com::{
                CoCreateInstance, CLSCTX_ALL,
            },
            System::Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
            Foundation::CloseHandle,
        },
    };

    let mut result: Vec<String> = Vec::new();

    unsafe {
        let enumerator: IMMDeviceEnumerator =
            match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                Ok(e) => e,
                Err(_) => return result,
            };

        let collection: IMMDeviceCollection =
            match enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) {
                Ok(c) => c,
                Err(_) => return result,
            };

        let count = match collection.GetCount() {
            Ok(n) => n,
            Err(_) => return result,
        };

        for i in 0..count {
            let device = match collection.Item(i) {
                Ok(d) => d,
                Err(_) => continue,
            };

            let manager: IAudioSessionManager2 = match device.Activate(CLSCTX_ALL, None) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let session_enum = match manager.GetSessionEnumerator() {
                Ok(e) => e,
                Err(_) => continue,
            };

            let sess_count = match session_enum.GetCount() {
                Ok(n) => n,
                Err(_) => continue,
            };

            for j in 0..sess_count {
                let ctrl: IAudioSessionControl = match session_enum.GetSession(j) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                let ctrl2: IAudioSessionControl2 = match ctrl.cast() {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                let pid = match ctrl2.GetProcessId() {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                if pid == 0 {
                    continue;
                }

                // Check if the session state is active (AudioSessionStateActive = 1)
                let state = match ctrl.GetState() {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                if state.0 != 1 {
                    continue;
                }

                // Resolve process name
                let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    Ok(h) => h,
                    Err(_) => continue,
                };
                let mut buf = vec![0u16; 1024];
                let mut len = buf.len() as u32;
                if QueryFullProcessImageNameW(
                    handle,
                    PROCESS_NAME_WIN32,
                    windows::core::PWSTR(buf.as_mut_ptr()),
                    &mut len,
                )
                .is_ok()
                {
                    let full_path = String::from_utf16_lossy(&buf[..len as usize]);
                    let stem = std::path::Path::new(&full_path)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Unknown")
                        .to_string();
                    if !result.contains(&stem) {
                        result.push(stem);
                    }
                }
                let _ = CloseHandle(handle);
            }
        }
    }

    result
}

#[cfg(target_os = "windows")]
pub fn perform_autopause() {
    // 1. Get all active rendering apps
    let active_render_apps = get_all_app_peaks();
    if active_render_apps.is_empty() {
        info!("Autopause: No active audio playback detected. Skipping pause.");
        return;
    }

    // 2. Get all active capture apps (mic usage)
    let active_capture_apps = get_active_capture_processes();

    info!("Autopause: Active render: {:?}, Active capture: {:?}", active_render_apps, active_capture_apps);

    // List of dedicated communication/meeting processes
    let dedicated_meeting_apps = [
        "teams", "zoom", "discord", "skype", "webex", "slack", "whatsapp", 
        "tg", "telegram", "phoneexperiencehost", "chime", "viber", 
        "signal", "element", "mumble", "ventrilo"
    ];

    // List of browser processes
    let browser_apps = [
        "chrome", "msedge", "firefox", "opera", "brave"
    ];

    // List of known media processes
    let media_apps = [
        "spotify", "vlc", "itunes", "music", "foobar2000", "wmplayer", 
        "netflix", "hulu", "prime", "deezer", "plex", "plexmediaplayer", 
        "tidals", "tidal", "potplayer", "mpc-hc", "mpc-hc64", "kmplayer", 
        "aimp", "winamp", "gpmdp"
    ];

    let mut has_meeting = false;
    let mut has_media = false;

    // Check rendering apps
    for app in &active_render_apps {
        let app_lower = app.to_lowercase();
        
        // If it's a dedicated meeting app, skip pause
        if dedicated_meeting_apps.iter().any(|m| app_lower.contains(m)) {
            info!("Autopause: Dedicated meeting app rendering audio: {}. Skipping pause.", app);
            has_meeting = true;
            break;
        }

        // If it's a browser, check if it's also using the mic
        if browser_apps.iter().any(|b| app_lower.contains(b)) {
            let is_using_mic = active_capture_apps.iter().any(|c| c.to_lowercase().contains(&app_lower));
            if is_using_mic {
                info!("Autopause: Browser is rendering audio and using mic (active call): {}. Skipping pause.", app);
                has_meeting = true;
                break;
            } else {
                has_media = true;
            }
        }

        // If it's a known media app, mark as media
        if media_apps.iter().any(|m| app_lower.contains(m)) {
            has_media = true;
        }
    }

    // Also check active capture apps: if any dedicated meeting app is capturing mic, skip pause
    for app in &active_capture_apps {
        let app_lower = app.to_lowercase();
        if dedicated_meeting_apps.iter().any(|m| app_lower.contains(m)) {
            info!("Autopause: Dedicated meeting app capturing mic: {}. Skipping pause.", app);
            has_meeting = true;
            break;
        }
    }

    if has_meeting {
        info!("Autopause: Active meeting/call detected. Skipping pause, but muting all audio output.");
        mute_all_render_endpoints();
        return;
    }

    if !has_media {
        info!("Autopause: No media/browser apps rendering audio. Skipping pause.");
        return;
    }

    info!("Autopause: Triggering media pause.");

    let mut smtc_paused_any = false;

    let smtc_run = || -> Result<bool, windows::core::Error> {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
        let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.get()?;
        let sessions = manager.GetSessions()?;
        let size = sessions.Size()?;
        let mut paused_count = 0;
        for i in 0..size {
            if let Ok(session) = sessions.GetAt(i) {
                if let Ok(op) = session.TryPauseAsync() {
                    if let Ok(success) = op.get() {
                        if success {
                            paused_count += 1;
                        }
                    }
                }
            }
        }
        Ok(paused_count > 0)
    };

    match smtc_run() {
        Ok(true) => {
            info!("Autopause: Successfully paused media sessions via WinRT SMTC.");
            smtc_paused_any = true;
        }
        Ok(false) => {
            info!("Autopause: No active SMTC media sessions could be paused. Will use keyboard fallback.");
        }
        Err(err) => {
            info!("Autopause: SMTC query failed: {:?}. Will use keyboard fallback.", err);
        }
    }

    if !smtc_paused_any {
        #[link(name = "user32")]
        extern "system" {
            fn keybd_event(b_vk: u8, b_scan: u8, dw_flags: u32, dw_extra_info: usize);
        }

        unsafe {
            const VK_MEDIA_PLAY_PAUSE: u8 = 0xB3;
            const KEYEVENTF_KEYUP: u32 = 2;
            keybd_event(VK_MEDIA_PLAY_PAUSE, 0, 0, 0);
            keybd_event(VK_MEDIA_PLAY_PAUSE, 0, KEYEVENTF_KEYUP, 0);
        }
    }
}

#[cfg(target_os = "windows")]
pub fn mute_all_render_endpoints() {
    use windows::{
        core::Interface,
        Win32::{
            Media::Audio::{
                eRender, IMMDeviceCollection, IMMDeviceEnumerator,
                MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
                Endpoints::IAudioEndpointVolume,
            },
            System::Com::{CoCreateInstance, CLSCTX_ALL},
        },
    };

    unsafe {
        let enumerator: IMMDeviceEnumerator =
            match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                Ok(e) => e,
                Err(_) => return,
            };

        let collection: IMMDeviceCollection =
            match enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) {
                Ok(c) => c,
                Err(_) => return,
            };

        let count = match collection.GetCount() {
            Ok(n) => n,
            Err(_) => return,
        };

        for i in 0..count {
            if let Ok(device) = collection.Item(i) {
                if let Ok(endpoint_volume) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
                    let _ = endpoint_volume.SetMute(true, std::ptr::null());
                }
            }
        }
        info!("Autopause: Muted all active audio rendering endpoints.");
    }
}

#[cfg(not(target_os = "windows"))]
mod stubs {
    pub fn get_active_app_peaks(_target_device: &str) -> Vec<String> {
        vec![]
    }

    pub fn get_all_app_peaks() -> Vec<String> {
        vec![]
    }

    pub fn get_active_capture_processes() -> Vec<String> {
        vec![]
    }

    pub fn mute_all_render_endpoints() {}

    pub fn perform_autopause() {}
}

#[cfg(not(target_os = "windows"))]
pub use stubs::*;
