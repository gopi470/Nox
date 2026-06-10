// bluetooth.rs – Bluetooth connection monitor
//
// Detection strategy (in priority order):
//
// Strategy 1: WASAPI – enumerate only ACTIVE audio endpoints.
//             A BT device only becomes an active WASAPI endpoint when it is
//             physically connected and the audio stack has claimed it.
//             This is the most reliable signal.
//
// Strategy 2: MMDEVAPI PnP presence – look for a SWD\MMDEVAPI entry with
//             FriendlyName matching the target AND Present = True.
//             When earbuds disconnect, Windows marks all their SWD\MMDEVAPI
//             entries as Present = False immediately (confirmed via testing).
//             All other BTHENUM entries (transport, avrcp, handsfree) stay
//             Present = True / Status = OK even when device is OFF, so we
//             explicitly exclude them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use log::{debug, info, warn};

#[cfg(target_os = "windows")]
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

const POLL_INTERVAL: Duration = Duration::from_secs(3);

use parking_lot::RwLock;

pub struct BluetoothMonitor {
    device_name: Arc<RwLock<String>>,
    connected: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
}

impl BluetoothMonitor {
    pub fn new(device_name: Arc<RwLock<String>>) -> Self {
        Self {
            device_name,
            connected: Arc::new(AtomicBool::new(false)),
            stop_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Spawn background polling thread.
    pub fn start(
        &self,
        on_connect: impl Fn() + Send + 'static,
        on_disconnect: impl Fn() + Send + 'static,
    ) {
        let device_name_lock = Arc::clone(&self.device_name);
        let connected = Arc::clone(&self.connected);
        let stop_flag = Arc::clone(&self.stop_flag);

        std::thread::Builder::new()
            .name("BTMonitor".into())
            .spawn(move || {
                #[cfg(target_os = "windows")]
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                }

                let mut last: Option<bool> = None;

                while !stop_flag.load(Ordering::SeqCst) {
                    let dev_name = device_name_lock.read().to_lowercase();
                    let now_connected = check_connected(&dev_name);

                    if Some(now_connected) != last {
                        let prev = last; // capture before updating
                        last = Some(now_connected);
                        connected.store(now_connected, Ordering::SeqCst);
                        if now_connected {
                            info!("Bluetooth CONNECTED");
                            on_connect();
                        } else if prev.is_some() {
                            // Only fire disconnect on a genuine connected→disconnected
                            // transition. Skip the initial unknown→false on startup.
                            info!("Bluetooth DISCONNECTED");
                            on_disconnect();
                        } else {
                            info!("Bluetooth startup state: disconnected (no callback)");
                        }
                    }

                    std::thread::sleep(POLL_INTERVAL);
                }
            })
            .expect("Failed to spawn BTMonitor thread");
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }
}

// ── Main detection logic ──────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn check_connected(target: &str) -> bool {
    // Strategy 1: WASAPI active audio endpoints — most precise signal
    match check_wasapi(target) {
        Some(true) => {
            debug!("Strategy 1 (WASAPI active endpoint): connected");
            return true;
        }
        Some(false) => {
            // WASAPI returned definitively false — device is not an active audio endpoint.
            // Fall through to Strategy 2 (e.g. connected but no audio app open yet).
        }
        None => {
            // WASAPI enumeration itself failed — skip to fallback.
            warn!("Strategy 1 (WASAPI): enumeration failed, falling back");
        }
    }

    // Strategy 2: MMDEVAPI PnP presence.
    // Windows marks SWD\MMDEVAPI entries as Present=False the moment earbuds
    // disconnect, even when BTHENUM entries linger as Present=True.
    match check_mmdevapi_present(target) {
        Some(true) => {
            debug!("Strategy 2 (MMDEVAPI Present): connected");
            true
        }
        _ => {
            debug!("All strategies: disconnected");
            false
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn check_connected(_target: &str) -> bool { false }

// ── Strategy 1: WASAPI active endpoints ──────────────────────────────────────

#[cfg(target_os = "windows")]
fn check_wasapi(target: &str) -> Option<bool> {
    use windows::{
        Win32::{
            Media::Audio::{
                IMMDeviceCollection, IMMDeviceEnumerator,
                MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
                eRender, eCapture,
            },
            System::Com::{CoTaskMemFree, STGM_READ},
            System::Com::StructuredStorage::PropVariantToStringAlloc,
            Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        },
    };

    let directions = [eRender, eCapture];

    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;

        for direction in directions {
            let collection: IMMDeviceCollection = match enumerator
                .EnumAudioEndpoints(direction, DEVICE_STATE_ACTIVE) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let count = match collection.GetCount() {
                Ok(n) => n,
                Err(_) => continue,
            };
            for i in 0..count {
                let device = match collection.Item(i) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let store = match device.OpenPropertyStore(STGM_READ) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let prop = match store.GetValue(&PKEY_Device_FriendlyName) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let psz_out = match PropVariantToStringAlloc(&prop) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let name = psz_out.to_string().unwrap_or_default().to_lowercase();
                CoTaskMemFree(Some(psz_out.0 as *const _));
                debug!("WASAPI active endpoint: {name}");
                if name.contains(target) {
                    return Some(true);
                }
            }
        }
        Some(false)
    }
}

// ── Strategy 2: MMDEVAPI PnP Present flag ────────────────────────────────────
//
// When earbuds disconnect, Windows immediately sets Present=False on all their
// SWD\MMDEVAPI audio endpoint entries. We look exclusively for those entries
// (InstanceId starts with "SWD\MMDEVAPI") and check Present=True.
// BTHENUM entries (transport, AVRCP, HFP) are intentionally excluded because
// they remain Present=True / Status=OK even when the device is powered off.

#[cfg(target_os = "windows")]
fn check_mmdevapi_present(target: &str) -> Option<bool> {
    use std::os::windows::process::CommandExt;
    let script = "Get-PnpDevice | \
         Where-Object { $_.InstanceId -like 'SWD\\MMDEVAPI*' -and \
                         $_.FriendlyName -like \"*$($env:TARGET_DEVICE)*\" -and \
                         $_.Present -eq $true } | \
         Measure-Object | Select-Object -ExpandProperty Count";
    let mut command = std::process::Command::new("powershell");
    command.creation_flags(0x08000000);
    command.env("TARGET_DEVICE", target);
    let output = command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    debug!("MMDEVAPI present count: '{s}'");
    let count: u32 = s.parse().ok()?;
    Some(count > 0)
}
