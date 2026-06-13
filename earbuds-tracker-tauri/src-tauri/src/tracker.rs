// tracker.rs – Core session state machine (mirrors tracker.py)
use chrono::{Local, NaiveDate, NaiveDateTime};
use std::time::{SystemTime, UNIX_EPOCH};
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
    start_dt: NaiveDateTime,
    last_tick: Instant,
    connected_secs: f64,
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
    battery_step: Arc<RwLock<u8>>,
    bt: Arc<BluetoothMonitor>,
    audio: Arc<AudioMonitor>,
    app_audio: Arc<AppAudioMonitor>,
    inner: Arc<Mutex<Inner>>,
    pub battery_cache: Arc<Mutex<Option<crate::spp::BatteryInfo>>>,
    // Callback invoked on any state change (set by Tauri main)
    pub on_state_change: Arc<Mutex<Option<Box<dyn Fn() + Send + 'static>>>>,
}

fn round_to_step(val: u8, step: u8) -> u8 {
    if step <= 1 {
        return val;
    }
    let half_step = step / 2;
    let rounded = ((val + half_step) / step) * step;
    rounded.min(100)
}

fn merge_battery_reading(
    previous: Option<crate::spp::BatteryInfo>,
    mut current: crate::spp::BatteryInfo,
) -> crate::spp::BatteryInfo {
    if let Some(prev) = previous {
        if current.left.is_none() {
            current.left = prev.left;
        }
        if current.right.is_none() {
            current.right = prev.right;
        }
        if current.case.is_none() {
            current.case = prev.case;
        }
    }
    current
}

impl Tracker {
    pub fn new(device_name: &str, battery_interval_secs: u64, battery_step: u8) -> Self {
        let dev_name = Arc::new(RwLock::new(device_name.to_string()));
        Self {
            device_name: dev_name.clone(),
            battery_interval_secs: Arc::new(RwLock::new(battery_interval_secs)),
            battery_step: Arc::new(RwLock::new(battery_step)),
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

    pub fn get_active_session_id(&self) -> Option<i64> {
        self.inner.lock().session.as_ref().map(|s| s.id)
    }

    pub fn set_battery_interval_secs(&self, secs: u64) {
        *self.battery_interval_secs.write() = secs;
    }

    pub fn get_battery_interval_secs(&self) -> u64 {
        *self.battery_interval_secs.read()
    }

    pub fn set_battery_step(&self, step: u8) {
        *self.battery_step.write() = step;
    }

    pub fn get_battery_step(&self) -> u8 {
        *self.battery_step.read()
    }

    pub fn force_query_battery(&self) -> Option<crate::spp::BatteryInfo> {
        let dev = self.get_device_name();
        if dev.is_empty() {
            return None;
        }
        self.record_query_log(
            "Battery query sent (Force)",
            format!("Polling {}", dev),
        );
        let previous_bat = self.battery_cache.lock().clone();
        if let Some(mut bat) = crate::spp::read_battery(&dev) {
            bat = merge_battery_reading(previous_bat, bat);
            let step = self.get_battery_step();
            if step > 1 {
                bat.left = bat.left.map(|v| round_to_step(v, step));
                bat.right = bat.right.map(|v| round_to_step(v, step));
                bat.case = bat.case.map(|v| round_to_step(v, step));
            }
            let updated_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            bat.updated_at = Some(updated_at);

            self.record_query_log(
                "Battery response received (Force)",
                format!("L={:?} R={:?} C={:?}", bat.left, bat.right, bat.case),
            );

            *self.battery_cache.lock() = Some(bat.clone());
            if let Some(sess_id) = self.get_active_session_id() {
                db::set_connect_battery(sess_id, bat.left, bat.right, bat.case);
            }
            call_notify(&self.on_state_change);
            Some(bat)
        } else {
            self.record_query_log(
                "Battery query failed (Force)",
                "No battery response received",
            );
            None
        }
    }

    pub fn record_query_log(&self, action: impl Into<String>, details: impl Into<String>) {
        let (session_id, session_start) = {
            let g = self.inner.lock();
            if let Some(s) = &g.session {
                (
                    Some(s.id),
                    Some(s.start_dt.format("%Y-%m-%dT%H:%M:%S").to_string()),
                )
            } else {
                (None, None)
            }
        };

        let Some(session_id) = session_id else { return; };
        let Some(session_start) = session_start else { return; };
        let event_ts = Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let action = action.into();
        let details = details.into();
        db::insert_query_log(session_id, &session_start, &event_ts, &action, &details);
    }

    pub fn get_query_log(&self) -> Vec<db::QueryLogRow> {
        db::get_query_log(200)
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
                if let Some(s) = g.session.as_mut() {
                    if s.play_start.is_none() {
                        s.play_start = Some(Instant::now());
                    }
                }
                call_notify(&notify_play);
            },
            move || {
                let mut g = inner_pause.lock();
                if let Some(s) = g.session.as_mut() {
                    if s.play_start.take().is_some() {
                        // Compensate for detection offsets: +5s grace period, -2s loud_count = +3s net overestimation.
                        // We deduct the 3.0s from the accumulated playback_secs.
                        if s.playback_secs >= 3.0 {
                            s.playback_secs -= 3.0;
                        } else {
                            s.playback_secs = 0.0;
                        }
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
                let now_instant = Instant::now();
                g.session = Some(Session {
                    id,
                    start_dt: now,
                    last_tick: now_instant,
                    connected_secs: 0.0,
                    play_start: None,
                    playback_secs: 0.0,
                });
                info!("Session opened id={id}");
                call_notify(&notify_conn);
                let current_dev = dev_name_lock.read().clone();
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
                        while bt_clone.is_connected() {
                            let previous_bat = cache_clone.lock().clone();
                            tracker_clone.record_query_log(
                                "Battery query sent",
                                format!("Polling {}", dev_for_bat),
                            );
                            if let Some(mut bat) = crate::spp::read_battery(&dev_for_bat) {
                                bat = merge_battery_reading(previous_bat, bat);
                                let step = tracker_clone.get_battery_step();
                                if step > 1 {
                                    bat.left = bat.left.map(|v| round_to_step(v, step));
                                    bat.right = bat.right.map(|v| round_to_step(v, step));
                                    bat.case = bat.case.map(|v| round_to_step(v, step));
                                }
                                let updated_at = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis() as u64;
                                bat.updated_at = Some(updated_at);

                                info!("SPP Battery Poll: L={:?} R={:?} C={:?} updated_at={}", bat.left, bat.right, bat.case, updated_at);
                                tracker_clone.record_query_log(
                                    "Battery response received",
                                    format!("L={:?} R={:?} C={:?}", bat.left, bat.right, bat.case),
                                );
                                *cache_clone.lock() = Some(bat.clone());
                                db::set_connect_battery(id, bat.left, bat.right, bat.case);
                                call_notify(&on_change_clone);
                            } else {
                                tracker_clone.record_query_log(
                                    "Battery query failed",
                                    "No battery response received",
                                );
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

                // Autopause on disconnect logic
                if crate::load_settings().autopause_enabled {
                    std::thread::spawn(|| {
                        std::thread::sleep(Duration::from_millis(500));
                        info!("Autopause: Device disconnected. Checking if we need to pause media.");
                        crate::app_audio::perform_autopause();
                    });
                }

                // Auto-exit on disconnect was removed so that the tracker remains running in the background to monitor future reconnect events.
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

fn current_durations_for_session(s: &Session) -> (f64, f64) {
    let now = Instant::now();
    let delta = now.checked_duration_since(s.last_tick).unwrap_or_default().as_secs_f64();
    let c = if delta < 3.0 { s.connected_secs + delta } else { s.connected_secs };
    let p = if let Some(ps) = s.play_start {
        if delta < 3.0 {
            let play_delta_start = if ps > s.last_tick { ps } else { s.last_tick };
            let play_delta = now.checked_duration_since(play_delta_start).unwrap_or_default().as_secs_f64();
            s.playback_secs + play_delta
        } else {
            s.playback_secs
        }
    } else {
        s.playback_secs
    };
    (c, p)
}

fn current_durations(g: &Inner) -> (f64, f64) {
    if let Some(s) = &g.session {
        current_durations_for_session(s)
    } else {
        (0.0, 0.0)
    }
}

fn finalise_session(g: &mut Inner) {
    if let Some(s) = g.session.take() {
        let now = Local::now().naive_local();
        let (c, p) = current_durations_for_session(&s);
        db::close_session(s.id, &now, c, p);
        db::add_to_daily(&now.date(), c, p);
        info!("Session closed id={}", s.id);
    }
}

fn schedule_live_write(inner: Arc<Mutex<Inner>>) {
    std::thread::spawn(move || {
        let mut live_write_counter = 0;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let mut g = inner.lock();
            if let Some(s) = &mut g.session {
                let now = Instant::now();
                let delta = now.checked_duration_since(s.last_tick).unwrap_or_default().as_secs_f64();
                
                if delta < 3.0 {
                    s.connected_secs += delta;
                    if let Some(ps) = s.play_start {
                        let play_delta_start = if ps > s.last_tick { ps } else { s.last_tick };
                        let play_delta = now.checked_duration_since(play_delta_start).unwrap_or_default().as_secs_f64();
                        s.playback_secs += play_delta;
                    }
                } else {
                    info!("Suspend/Resume detected: gap of {:.2}s ignored", delta);
                    if s.play_start.is_some() {
                        s.play_start = Some(now);
                    }
                }
                
                s.last_tick = now;

                live_write_counter += 1;
                if live_write_counter >= LIVE_WRITE_SECS {
                    live_write_counter = 0;
                    let c = s.connected_secs;
                    let p = s.playback_secs;
                    db::update_session_live(s.id, c, p);
                }
            } else {
                break;
            }
        }
    });
}



// Need this for month.with_day()
use chrono::Datelike;
