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

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const SILENCE_THRESHOLD: f32 = 0.001;
const GRACE_CHECKS: u32 = 5;

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

#[cfg(not(target_os = "windows"))]
pub fn get_active_app_peaks(_target_device: &str) -> Vec<String> {
    vec![]
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

#[cfg(not(target_os = "windows"))]
pub fn get_all_app_peaks() -> Vec<String> {
    vec![]
}
