// tracker.rs – Core session state machine (mirrors tracker.py)
use chrono::{Local, NaiveDate, NaiveDateTime};
use log::info;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::bluetooth::BluetoothMonitor;
use crate::audio::AudioMonitor;
use crate::app_audio::AppAudioMonitor;
use crate::db;

const LIVE_WRITE_SECS: u64 = 30;

#[derive(Clone, serde::Serialize)]
pub struct Snapshot {
    pub connected: bool,
    pub playing: bool,
    pub sess_conn: f64,
    pub sess_play: f64,
    pub today: db::StatsPeriod,
    pub week: db::StatsPeriod,
    pub month: db::StatsPeriod,
    pub lifetime: db::StatsPeriod,
}

struct Session {
    id: i64,
    start: Instant,
    start_dt: NaiveDateTime,
    play_start: Option<Instant>,
    playback_secs: f64,
}

struct Inner {
    session: Option<Session>,
}

use parking_lot::RwLock;

pub struct Tracker {
    device_name: Arc<RwLock<String>>,
    battery_interval_secs: Arc<RwLock<u64>>,
    bt: Arc<BluetoothMonitor>,
    audio: Arc<AudioMonitor>,
    app_audio: Arc<AppAudioMonitor>,
    inner: Arc<Mutex<Inner>>,
    pub battery_cache: Arc<Mutex<Option<crate::spp::BatteryInfo>>>,
    // Callback invoked on any state change (set by Tauri main)
    pub on_state_change: Arc<Mutex<Option<Box<dyn Fn() + Send + 'static>>>>,
}

impl Tracker {
    pub fn new(device_name: &str, battery_interval_secs: u64) -> Self {
        let dev_name = Arc::new(RwLock::new(device_name.to_string()));
        Self {
            device_name: dev_name.clone(),
            battery_interval_secs: Arc::new(RwLock::new(battery_interval_secs)),
            bt: Arc::new(BluetoothMonitor::new(dev_name.clone())),
            audio: Arc::new(AudioMonitor::new(dev_name.clone())),
            app_audio: Arc::new(AppAudioMonitor::new(dev_name)),
            inner: Arc::new(Mutex::new(Inner { session: None })),
            battery_cache: Arc::new(Mutex::new(None)),
            on_state_change: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_device_name(&self, new_name: &str) {
        *self.device_name.write() = new_name.to_string();
    }

    pub fn get_device_name(&self) -> String {
        self.device_name.read().clone()
    }

    pub fn set_battery_interval_secs(&self, secs: u64) {
        *self.battery_interval_secs.write() = secs;
    }

    pub fn get_battery_interval_secs(&self) -> u64 {
        *self.battery_interval_secs.read()
    }

    pub fn start(self: &Arc<Self>, app_handle: tauri::AppHandle) {
        db::init_db();

        // ── Per-app audio tracking ────────────────────────────────────────────
        {
            let inner_for_sess = Arc::clone(&self.inner);
            self.app_audio.start(
                // get_session_id: returns active session id or None
                move || inner_for_sess.lock().session.as_ref().map(|s| s.id),
                // on_start: insert DB event row, return its id
                |session_id, process_name| db::open_app_event(session_id, process_name.as_str()),
                // on_stop: close the DB event row
                |event_id, process_name| db::close_app_event(event_id, process_name.as_str()),
            );
        }

        // ── Audio callbacks ───────────────────────────────────────────────
        let inner_play = Arc::clone(&self.inner);
        let notify_play = Arc::clone(&self.on_state_change);
        let inner_pause = Arc::clone(&self.inner);
        let notify_pause = Arc::clone(&self.on_state_change);

        self.audio.start(
            move || {
                let mut g = inner_play.lock();
                if g.session.is_some() {
                    let s = g.session.as_mut().unwrap();
                    if s.play_start.is_none() {
                        s.play_start = Some(Instant::now());
                    }
                }
                call_notify(&notify_play);
            },
            move || {
                let mut g = inner_pause.lock();
                if let Some(s) = g.session.as_mut() {
                    if let Some(ps) = s.play_start.take() {
                        s.playback_secs += ps.elapsed().as_secs_f64();
                    }
                }
                call_notify(&notify_pause);
            },
        );

        // ── BT callbacks ──────────────────────────────────────────────────
        let inner_conn = Arc::clone(&self.inner);
        let notify_conn = Arc::clone(&self.on_state_change);
        let inner_disc = Arc::clone(&self.inner);
        let notify_disc = Arc::clone(&self.on_state_change);
        let dev_name_lock      = Arc::clone(&self.device_name);
        let dev_name_lock_disc = Arc::clone(&self.device_name);

        let app_handle_conn = app_handle.clone();
        let app_handle_disc = app_handle.clone();
        let tracker_conn = Arc::clone(self);
        let tracker_disc = Arc::clone(self);

        self.bt.start(
            move || {
                let now = Local::now().naive_local();
                let id = db::open_session(&now);
                let mut g = inner_conn.lock();
                g.session = Some(Session {
                    id,
                    start: Instant::now(),
                    start_dt: now,
                    play_start: None,
                    playback_secs: 0.0,
                });
                info!("Session opened id={id}");
                call_notify(&notify_conn);
                let current_dev = dev_name_lock.read().clone();
                run_event_script("connect", &current_dev);
                schedule_live_write(Arc::clone(&inner_conn));

                // Start background battery polling loop
                {
                    let dev_for_bat = current_dev.clone();
                    let cache_clone = Arc::clone(&tracker_conn.battery_cache);
                    let bt_clone = Arc::clone(&tracker_conn.bt);
                    let on_change_clone = Arc::clone(&tracker_conn.on_state_change);
                    let tracker_clone = Arc::clone(&tracker_conn);
                    std::thread::spawn(move || {
                        // Wait a short bit after connection to let SPP settle
                        std::thread::sleep(Duration::from_secs(2));
                        let mut db_updated = false;
                        while bt_clone.is_connected() {
                            if let Some(bat) = crate::spp::read_battery(&dev_for_bat) {
                                info!("SPP Battery Poll: L={:?} R={:?} C={:?}", bat.left, bat.right, bat.case);
                                *cache_clone.lock() = Some(bat.clone());
                                if !db_updated {
                                    db::set_connect_battery(id, bat.left, bat.right, bat.case);
                                    db_updated = true;
                                }
                                call_notify(&on_change_clone);
                            }
                            // Poll according to configured interval
                            let mut elapsed = 0;
                            while bt_clone.is_connected() && elapsed < tracker_clone.get_battery_interval_secs() {
                                std::thread::sleep(Duration::from_secs(1));
                                elapsed += 1;
                            }
                        }
                        // Clear cache on disconnect
                        *cache_clone.lock() = None;
                    });
                }

                // Hide window when earbuds connect
                use tauri::Manager;
                if let Some(win) = app_handle_conn.get_webview_window("main") {
                    win.hide().ok();
                }
            },
            move || {
                // Read battery at disconnect BEFORE finalising the session
                {
                    let last_bat = tracker_disc.battery_cache.lock().clone();
                    if let Some(bat) = last_bat {
                        let g = inner_disc.lock();
                        if let Some(sess) = g.session.as_ref() {
                            db::set_disconnect_battery(sess.id, bat.left, bat.right, bat.case);
                        }
                    }
                }
                let mut g = inner_disc.lock();
                finalise_session(&mut g);
                call_notify(&notify_disc);
                run_event_script("disconnect", "");

                // Start 5-second automatic exit on disconnect,
                // but only if the user hasn't manually opened the window.
                let app_handle_exit = app_handle_disc.clone();
                let tracker_exit = Arc::clone(&tracker_disc);
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(5));
                    if !tracker_exit.is_connected() {
                        use tauri::Manager;
                        let win_visible = app_handle_exit
                            .get_webview_window("main")
                            .and_then(|w| w.is_visible().ok())
                            .unwrap_or(false);
                        if win_visible {
                            // User manually opened the window — respect that, don't exit.
                            info!("Disconnected but window is visible (user override). Staying alive.");
                        } else {
                            info!("Still disconnected after 5 s (background mode). Exiting.");
                            app_handle_exit.exit(0);
                        }
                    }
                });
            },
        );
    }

    pub fn is_connected(&self) -> bool { self.bt.is_connected() }
    pub fn is_playing(&self) -> bool   { self.audio.is_playing() }

    pub fn get_snapshot(&self) -> Snapshot {
        let (sess_conn, sess_play) = {
            let g = self.inner.lock();
            current_durations(&g)
        };
        let connected = self.bt.is_connected();
        let playing   = self.audio.is_playing() && connected;

        let today      = chrono::Local::now().date_naive();
        let week_start = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
        let month_start= today.with_day(1).unwrap_or(today);

        let mut today_s = db::get_stats_for_range(&today, &today);
        let mut week_s  = db::get_stats_for_range(&week_start, &today);
        let mut month_s = db::get_stats_for_range(&month_start, &today);
        let mut life_s  = db::get_lifetime_stats();

        // Add unsaved current session
        for s in [&mut today_s, &mut week_s, &mut month_s, &mut life_s] {
            s.connected += sess_conn;
            s.playback  += sess_play;
        }

        Snapshot { connected, playing, sess_conn, sess_play,
                   today: today_s, week: week_s, month: month_s, lifetime: life_s }
    }

    pub fn get_recent_sessions(&self) -> Vec<db::SessionRow> {
        db::get_recent_sessions(200)
    }

    pub fn reset_all(&self) {
        db::reset_all_data();
        call_notify(&self.on_state_change);
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn call_notify(cb: &Arc<Mutex<Option<Box<dyn Fn() + Send + 'static>>>>) {
    if let Some(f) = cb.lock().as_ref() { f(); }
}

fn current_durations(g: &Inner) -> (f64, f64) {
    if let Some(s) = &g.session {
        let c = s.start.elapsed().as_secs_f64();
        let p = s.playback_secs + s.play_start.map(|ps| ps.elapsed().as_secs_f64()).unwrap_or(0.0);
        (c, p)
    } else {
        (0.0, 0.0)
    }
}

fn finalise_session(g: &mut Inner) {
    if let Some(s) = g.session.take() {
        let now = Local::now().naive_local();
        let c = s.start.elapsed().as_secs_f64();
        let p = s.playback_secs + s.play_start.map(|ps| ps.elapsed().as_secs_f64()).unwrap_or(0.0);
        db::close_session(s.id, &now, c, p);
        db::add_to_daily(&now.date(), c, p);
        info!("Session closed id={}", s.id);
    }
}

fn schedule_live_write(inner: Arc<Mutex<Inner>>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(LIVE_WRITE_SECS));
            let g = inner.lock();
            if let Some(s) = &g.session {
                let (c, p) = current_durations(&g);
                db::update_session_live(s.id, c, p);
            } else {
                break;
            }
        }
    });
}

fn run_event_script(event_type: &str, _device: &str) {
    use std::path::Path;
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_default();
    let parent = exe_dir.parent().unwrap_or(&exe_dir).to_path_buf();

    let script_name = format!("on_{event_type}");
    let exts = [".ahk", ".bat", ".cmd", ".ps1"];

    for dir in [&parent, &exe_dir] {
        for ext in &exts {
            let path = dir.join(format!("{script_name}{ext}"));
            if path.exists() {
                info!("Running {event_type} script: {}", path.display());
                if *ext == ".ahk" {
                    let ahk_paths = [
                        r"C:\Program Files\AutoHotkey\v2\AutoHotkey64.exe",
                        r"C:\Program Files\AutoHotkey\AutoHotkey.exe",
                    ];
                    for ahk in &ahk_paths {
                        if Path::new(ahk).exists() {
                            let mut cmd = std::process::Command::new(ahk);
                            #[cfg(target_os = "windows")]
                            {
                                use std::os::windows::process::CommandExt;
                                cmd.creation_flags(0x08000000);
                            }
                            cmd.arg(&path).spawn().ok();
                            return;
                        }
                    }
                } else {
                    let mut cmd = std::process::Command::new("cmd");
                    #[cfg(target_os = "windows")]
                    {
                        use std::os::windows::process::CommandExt;
                        cmd.creation_flags(0x08000000);
                    }
                    cmd.args(["/C", &path.to_string_lossy()]).spawn().ok();
                }
                return;
            }
        }
    }
}

// Need this for month.with_day()
use chrono::Datelike;
