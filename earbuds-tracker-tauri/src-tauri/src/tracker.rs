// tracker.rs – Core session state machine (mirrors tracker.py)
use chrono::{Local, NaiveDateTime};
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
    mac_address: Arc<RwLock<Option<String>>>,
    protocol_mode: Arc<RwLock<String>>,
    brand: Arc<RwLock<String>>,
    battery_interval_secs: Arc<RwLock<u64>>,
    battery_step: Arc<RwLock<u8>>,
    bt: Arc<BluetoothMonitor>,
    audio: Arc<AudioMonitor>,
    app_audio: Arc<AppAudioMonitor>,
    inner: Arc<Mutex<Inner>>,
    pub battery_cache: Arc<Mutex<Option<crate::spp::BatteryInfo>>>,
    // Callback invoked on any state change (set by Tauri main)
    pub on_state_change: Arc<Mutex<Option<Box<dyn Fn() + Send + 'static>>>>,
    // Callback invoked when first-try protocol discovery completes — arg is "proprietary" or "standard"
    pub on_protocol_discovered: Arc<Mutex<Option<Box<dyn Fn(String) + Send + 'static>>>>,
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
    pub fn new(device_name: &str, mac_address: Option<String>, brand: &str, protocol_mode: &str, battery_interval_secs: u64, battery_step: u8) -> Self {
        let dev_name = Arc::new(RwLock::new(device_name.to_string()));
        Self {
            device_name: dev_name.clone(),
            mac_address: Arc::new(RwLock::new(mac_address)),
            protocol_mode: Arc::new(RwLock::new(protocol_mode.to_string())),
            brand: Arc::new(RwLock::new(brand.to_string())),
            battery_interval_secs: Arc::new(RwLock::new(battery_interval_secs)),
            battery_step: Arc::new(RwLock::new(battery_step)),
            bt: Arc::new(BluetoothMonitor::new(dev_name.clone())),
            audio: Arc::new(AudioMonitor::new(dev_name.clone())),
            app_audio: Arc::new(AppAudioMonitor::new(dev_name)),
            inner: Arc::new(Mutex::new(Inner { session: None })),
            battery_cache: Arc::new(Mutex::new(None)),
            on_state_change: Arc::new(Mutex::new(None)),
            on_protocol_discovered: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_device_name(&self, new_name: &str) {
        let old_name = self.get_device_name();
        if old_name != new_name {
            {
                let mut g = self.inner.lock();
                finalise_session(&mut g);
            }
            *self.battery_cache.lock() = None;
            self.bt.reset_connection_state();
            *self.device_name.write() = new_name.to_string();
            call_notify(&self.on_state_change);
        }
    }

    pub fn get_device_name(&self) -> String {
        self.device_name.read().clone()
    }

    pub fn set_mac_address(&self, new_mac: Option<String>) {
        *self.mac_address.write() = new_mac;
    }

    pub fn get_mac_address(&self) -> Option<String> {
        self.mac_address.read().clone()
    }

    pub fn set_protocol_mode(&self, new_mode: &str) {
        *self.protocol_mode.write() = new_mode.to_string();
    }

    pub fn get_protocol_mode(&self) -> String {
        self.protocol_mode.read().clone()
    }

    pub fn set_brand(&self, new_brand: &str) {
        *self.brand.write() = new_brand.to_string();
    }

    pub fn get_brand(&self) -> String {
        self.brand.read().clone()
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
        let mac = self.get_mac_address();
        let brand = self.get_brand();
        let protocol = self.get_protocol_mode();
        if dev.is_empty() {
            return None;
        }
        let start_sess_id = self.get_active_session_id();
        self.record_query_log(
            "Battery query sent (Force)",
            format!("Polling {}", dev),
        );
        let previous_bat = self.battery_cache.lock().clone();
        let (bat_opt, effective_mode) = crate::spp::read_battery(&dev, mac.as_deref(), &brand, &protocol);
        
        // If device changed during query, abort immediately without updating cache or DB
        if self.get_device_name() != dev {
            return None;
        }

        // If auto-discovery just ran, persist the discovered method
        if protocol == "auto" {
            self.set_protocol_mode(effective_mode);
            if let Some(ref cb) = *self.on_protocol_discovered.lock() {
                cb(effective_mode.to_string());
            }
        }
        if let Some(mut bat) = bat_opt {
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

            let log_details = if bat.right.is_none() && bat.case.is_none() {
                format!("Battery={:?}", bat.left)
            } else {
                format!("L={:?} R={:?} C={:?}", bat.left, bat.right, bat.case)
            };
            self.record_query_log(
                "Battery response received (Force)",
                log_details,
            );

            *self.battery_cache.lock() = Some(bat.clone());
            if let Some(sess_id) = self.get_active_session_id() {
                if Some(sess_id) == start_sess_id {
                    db::set_connect_battery(sess_id, bat.left, bat.right, bat.case);
                }
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

    pub fn record_query_log_for_session(&self, dev: &str, session_id: i64, session_start: &str, action: impl Into<String>, details: impl Into<String>) {
        let event_ts = Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let action = action.into();
        let details = details.into();
        db::insert_query_log(dev, session_id, session_start, &event_ts, &action, &details);
    }

    pub fn record_query_log(&self, action: impl Into<String>, details: impl Into<String>) {
        let (session_id, session_start) = {
            let g = self.inner.lock();
            if let Some(s) = &g.session {
                (
                    s.id,
                    s.start_dt.format("%Y-%m-%dT%H:%M:%S").to_string(),
                )
            } else {
                (0, "".to_string())
            }
        };

        let event_ts = Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let action = action.into();
        let details = details.into();
        let dev = self.get_device_name();
        db::insert_query_log(&dev, session_id, &session_start, &event_ts, &action, &details);
    }

    pub fn get_query_log(&self) -> Vec<db::QueryLogRow> {
        let dev = self.get_device_name();
        db::get_query_log(&dev, 200)
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

        let app_handle_conn = app_handle.clone();
        let tracker_conn = Arc::clone(self);
        let tracker_disc = Arc::clone(self);

        self.bt.start(
            move || {
                let now = Local::now().naive_local();
                let current_dev = dev_name_lock.read().clone();
                let id = db::open_session(&now, &current_dev);
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
                schedule_live_write(Arc::clone(&inner_conn));

                // Start background battery polling loop
                {
                    let cache_clone = Arc::clone(&tracker_conn.battery_cache);
                    let bt_clone = Arc::clone(&tracker_conn.bt);
                    let on_change_clone = Arc::clone(&tracker_conn.on_state_change);
                    let tracker_clone = Arc::clone(&tracker_conn);
                    let session_dev_name = current_dev.clone();
                    let session_id = id;
                    let session_start_str = now.format("%Y-%m-%dT%H:%M:%S").to_string();
                    std::thread::spawn(move || {
                        // Wait a short bit after connection to let SPP settle
                        std::thread::sleep(Duration::from_secs(2));
                        while bt_clone.is_connected() {
                            if tracker_clone.get_device_name() != session_dev_name || tracker_clone.get_active_session_id() != Some(session_id) {
                                break;
                            }
                            let previous_bat = cache_clone.lock().clone();
                            let current_dev = tracker_clone.get_device_name();
                            let current_mac = tracker_clone.get_mac_address();
                            let current_brand = tracker_clone.get_brand();
                            let current_proto = tracker_clone.get_protocol_mode();
                            tracker_clone.record_query_log_for_session(
                                &session_dev_name,
                                session_id,
                                &session_start_str,
                                "Battery query sent",
                                format!("Polling {}", session_dev_name),
                            );
                            let (bat_opt, effective_mode) = crate::spp::read_battery(
                                &current_dev,
                                current_mac.as_deref(),
                                &current_brand,
                                &current_proto,
                            );
                            // If device changed during query, abort immediately
                            if tracker_clone.get_device_name() != session_dev_name || tracker_clone.get_active_session_id() != Some(session_id) {
                                break;
                            }
                            // First-try discovery: persist the working method to the profile
                            if current_proto == "auto" {
                                tracker_clone.set_protocol_mode(effective_mode);
                                if let Some(ref cb) = *tracker_clone.on_protocol_discovered.lock() {
                                    cb(effective_mode.to_string());
                                }
                            }
                            if let Some(mut bat) = bat_opt {
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
                                let log_details = if bat.right.is_none() && bat.case.is_none() {
                                    format!("Battery={:?}", bat.left)
                                } else {
                                    format!("L={:?} R={:?} C={:?}", bat.left, bat.right, bat.case)
                                };
                                tracker_clone.record_query_log_for_session(
                                    &session_dev_name,
                                    session_id,
                                    &session_start_str,
                                    "Battery response received",
                                    log_details,
                                );
                                *cache_clone.lock() = Some(bat.clone());
                                db::set_connect_battery(session_id, bat.left, bat.right, bat.case);
                                call_notify(&on_change_clone);
                            } else {
                                tracker_clone.record_query_log_for_session(
                                    &session_dev_name,
                                    session_id,
                                    &session_start_str,
                                    "Battery query failed",
                                    "No battery response received",
                                );
                            }
                            // Poll according to configured interval
                            let mut elapsed = 0;
                            while bt_clone.is_connected() && elapsed < tracker_clone.get_battery_interval_secs() {
                                if tracker_clone.get_device_name() != session_dev_name || tracker_clone.get_active_session_id() != Some(session_id) {
                                    break;
                                }
                                std::thread::sleep(Duration::from_secs(1));
                                elapsed += 1;
                            }
                        }
                        // Clear cache on disconnect only if it's still the same device
                        if tracker_clone.get_device_name() == session_dev_name {
                            *cache_clone.lock() = None;
                        }
                    });
                }

                // Hide window when earbuds connect, except for the very first connection/session
                // of a newly created profile so the user can observe the initial telemetry.
                use tauri::Manager;
                let dev_name = tracker_conn.get_device_name();
                let is_first_session = crate::db::get_session_count_for_device(&dev_name) <= 1;
                if !is_first_session {
                    if let Some(win) = app_handle_conn.get_webview_window("main") {
                        win.hide().ok();
                    }
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

        let dev = self.get_device_name();
        let mut today_s = db::get_stats_for_range(&dev, &today, &today);
        let mut week_s  = db::get_stats_for_range(&dev, &week_start, &today);
        let mut month_s = db::get_stats_for_range(&dev, &month_start, &today);
        let mut life_s  = db::get_lifetime_stats(&dev);

        // Add unsaved current session
        for s in [&mut today_s, &mut week_s, &mut month_s, &mut life_s] {
            s.connected += sess_conn;
            s.playback  += sess_play;
        }

        Snapshot { connected, playing, sess_conn, sess_play,
                   today: today_s, week: week_s, month: month_s, lifetime: life_s }
    }

    pub fn get_recent_sessions(&self) -> Vec<db::SessionRow> {
        db::get_recent_sessions(&self.get_device_name(), 200)
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
