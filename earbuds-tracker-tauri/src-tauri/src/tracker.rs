// tracker.rs – Core session state machine (mirrors tracker.py)
use chrono::{Local, NaiveDate, NaiveDateTime};
use log::info;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::bluetooth::BluetoothMonitor;
use crate::audio::AudioMonitor;
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
    bt: Arc<BluetoothMonitor>,
    audio: Arc<AudioMonitor>,
    inner: Arc<Mutex<Inner>>,
    // Callback invoked on any state change (set by Tauri main)
    pub on_state_change: Arc<Mutex<Option<Box<dyn Fn() + Send + 'static>>>>,
}

impl Tracker {
    pub fn new(device_name: &str) -> Self {
        let dev_name = Arc::new(RwLock::new(device_name.to_string()));
        Self {
            device_name: dev_name.clone(),
            bt: Arc::new(BluetoothMonitor::new(dev_name.clone())),
            audio: Arc::new(AudioMonitor::new(dev_name)),
            inner: Arc::new(Mutex::new(Inner { session: None })),
            on_state_change: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_device_name(&self, new_name: &str) {
        *self.device_name.write() = new_name.to_string();
    }

    pub fn start(self: &Arc<Self>, app_handle: tauri::AppHandle) {
        db::init_db();

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
        let dev_name_lock = Arc::clone(&self.device_name);

        let app_handle_conn = app_handle.clone();
        let app_handle_disc = app_handle.clone();
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

                // Hide window when earbuds connect
                use tauri::Manager;
                if let Some(win) = app_handle_conn.get_webview_window("main") {
                    win.hide().ok();
                }
            },
            move || {
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
                            std::process::Command::new(ahk).arg(&path).spawn().ok();
                            return;
                        }
                    }
                } else {
                    std::process::Command::new("cmd").args(["/C", &path.to_string_lossy()]).spawn().ok();
                }
                return;
            }
        }
    }
}

// Need this for month.with_day()
use chrono::Datelike;
