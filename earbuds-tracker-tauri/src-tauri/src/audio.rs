// audio.rs – WASAPI peak-level audio monitor (mirrors audio_monitor.py)
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use log::{debug, info};

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const SILENCE_THRESHOLD: f32 = 0.001;
const GRACE_CHECKS: u32 = 8;

use parking_lot::RwLock;

pub struct AudioMonitor {
    device_name: Arc<RwLock<String>>,
    playing: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
}

impl AudioMonitor {
    pub fn new(device_name: Arc<RwLock<String>>) -> Self {
        Self {
            device_name,
            playing: Arc::new(AtomicBool::new(false)),
            stop_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::SeqCst)
    }

    pub fn start(
        &self,
        on_play: impl Fn() + Send + 'static,
        on_pause: impl Fn() + Send + 'static,
    ) {
        let device_name_lock = Arc::clone(&self.device_name);
        let playing = Arc::clone(&self.playing);
        let stop_flag = Arc::clone(&self.stop_flag);

        std::thread::Builder::new()
            .name("AudioMonitor".into())
            .spawn(move || {
                #[cfg(target_os = "windows")]
                unsafe {
                    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                }
                let mut silent_count: u32 = 0;
                let mut loud_count: u32 = 0;
                while !stop_flag.load(Ordering::SeqCst) {
                    let dev_name = device_name_lock.read().to_lowercase();
                    let peak = get_peak_level(&dev_name).unwrap_or(0.0);
                    if peak > SILENCE_THRESHOLD {
                        silent_count = 0;
                        loud_count += 1;
                        if !playing.load(Ordering::SeqCst) && loud_count >= 2 {
                            playing.store(true, Ordering::SeqCst);
                            info!("Audio PLAYING (peak={peak:.4})");
                            on_play();
                        }
                    } else {
                        silent_count += 1;
                        loud_count = 0;
                        if playing.load(Ordering::SeqCst) && silent_count >= GRACE_CHECKS {
                            playing.store(false, Ordering::SeqCst);
                            info!("Audio PAUSED after {silent_count} silent checks");
                            on_pause();
                        }
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
            })
            .expect("Failed to spawn AudioMonitor thread");
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }
}

#[cfg(target_os = "windows")]
fn get_peak_level(target: &str) -> Option<f32> {
    use windows::{
        core::{Interface, PWSTR},
        Win32::{
            Media::Audio::{
                Endpoints::IAudioMeterInformation,
                IMMDeviceCollection, IMMDeviceEnumerator, MMDeviceEnumerator,
                eRender, DEVICE_STATE_ACTIVE, IAudioSessionManager2,
            },
            System::Com::{CoCreateInstance, CLSCTX_ALL, CoTaskMemFree, STGM_READ},
            System::Com::StructuredStorage::PropVariantToStringAlloc,
            Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        },
    };
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let collection: IMMDeviceCollection = enumerator
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .ok()?;
        let count = collection.GetCount().ok()?;
        let mut max_peak: f32 = 0.0;
        for i in 0..count {
            let Ok(device) = collection.Item(i) else { continue };
            let Ok(store) = device.OpenPropertyStore(STGM_READ) else { continue };
            let Ok(prop) = store.GetValue(&PKEY_Device_FriendlyName) else { continue };
            let Ok(psz_out) = PropVariantToStringAlloc(&prop) else { continue };
            let name = psz_out.to_string().unwrap_or_default().to_lowercase();
            CoTaskMemFree(Some(psz_out.0 as *const _));

            if !name.contains(target) { continue; }
            
            let mut device_peak: f32 = 0.0;
            if let Ok(iface) = device.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None) {
                if let Ok(peak) = iface.GetPeakValue() {
                    device_peak = peak;
                }
            }

            let mut session_peak: f32 = 0.0;
            if let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) {
                if let Ok(session_enum) = manager.GetSessionEnumerator() {
                    if let Ok(sess_count) = session_enum.GetCount() {
                        for j in 0..sess_count {
                            let Ok(ctrl) = session_enum.GetSession(j) else { continue };
                            let Ok(meter) = ctrl.cast::<IAudioMeterInformation>() else { continue };
                            if let Ok(peak) = meter.GetPeakValue() {
                                if peak > session_peak {
                                    session_peak = peak;
                                }
                            }
                        }
                    }
                }
            }

            let peak = if device_peak > session_peak { device_peak } else { session_peak };
            debug!("Endpoint {name:?} peak={peak:.5} (device={device_peak:.5}, sessions={session_peak:.5})");
            if peak > max_peak {
                max_peak = peak;
            }
        }
        Some(max_peak)
    }
}

#[cfg(not(target_os = "windows"))]
fn get_peak_level(_target: &str) -> Option<f32> { Some(0.0) }
