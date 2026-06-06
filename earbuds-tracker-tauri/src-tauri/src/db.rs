// db.rs – SQLite persistence layer (mirrors database.py)
use chrono::{Local, NaiveDate, NaiveDateTime};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use rusqlite::{params, Connection, Result};
use std::path::PathBuf;

static DB_PATH: OnceCell<PathBuf> = OnceCell::new();
static CONN: OnceCell<Mutex<Connection>> = OnceCell::new();

fn db_path() -> &'static PathBuf {
    DB_PATH.get_or_init(|| {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
        let dir = PathBuf::from(appdata).join("EarbudsTracker");
        std::fs::create_dir_all(&dir).ok();
        dir.join("tracker.db")
    })
}

fn conn() -> &'static Mutex<Connection> {
    CONN.get_or_init(|| {
        let c = Connection::open(db_path()).expect("Cannot open SQLite DB");
        c.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS sessions (
                 id             INTEGER PRIMARY KEY AUTOINCREMENT,
                 session_start  TEXT NOT NULL,
                 session_end    TEXT,
                 connected_secs REAL NOT NULL DEFAULT 0,
                 playback_secs  REAL NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS idx_sessions_start ON sessions(session_start);
             CREATE TABLE IF NOT EXISTS daily_stats (
                 day            TEXT PRIMARY KEY,
                 connected_secs REAL NOT NULL DEFAULT 0,
                 playback_secs  REAL NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS app_audio_events (
                 id             INTEGER PRIMARY KEY AUTOINCREMENT,
                 session_id     INTEGER NOT NULL,
                 process_name   TEXT NOT NULL,
                 start_ts       TEXT NOT NULL,
                 end_ts         TEXT,
                 duration_secs  REAL
             );
             CREATE INDEX IF NOT EXISTS idx_app_events_session
                 ON app_audio_events(session_id);",
        )
        .expect("DB init failed");
        // ── Column migrations (safe on existing DBs) ────────────────────────
        let migrations = [
            "ALTER TABLE sessions ADD COLUMN bat_left_connect   INTEGER",
            "ALTER TABLE sessions ADD COLUMN bat_right_connect  INTEGER",
            "ALTER TABLE sessions ADD COLUMN bat_case_connect   INTEGER",
            "ALTER TABLE sessions ADD COLUMN bat_left_disc      INTEGER",
            "ALTER TABLE sessions ADD COLUMN bat_right_disc     INTEGER",
            "ALTER TABLE sessions ADD COLUMN bat_case_disc      INTEGER",
            "ALTER TABLE sessions ADD COLUMN firmware           TEXT",
            "ALTER TABLE sessions ADD COLUMN notes              TEXT",
            "ALTER TABLE sessions ADD COLUMN interrupted        INTEGER DEFAULT 0",
        ];
        for sql in &migrations {
            c.execute(sql, []).ok(); // silently ignored if column already exists
        }
        Mutex::new(c)
    })
}

pub fn init_db() {
    let _ = conn(); // trigger OnceCell init
    cleanup_unclosed_sessions();
}

// ── Session management ──────────────────────────────────────────────────────

pub fn open_session(start: &NaiveDateTime) -> i64 {
    let db = conn().lock();
    db.execute(
        "INSERT INTO sessions (session_start) VALUES (?1)",
        params![start.format("%Y-%m-%dT%H:%M:%S").to_string()],
    )
    .expect("open_session failed");
    db.last_insert_rowid()
}

pub fn close_session(id: i64, end: &NaiveDateTime, connected: f64, playback: f64) {
    let db = conn().lock();
    db.execute(
        "UPDATE sessions SET session_end=?1, connected_secs=?2, playback_secs=?3 WHERE id=?4",
        params![end.format("%Y-%m-%dT%H:%M:%S").to_string(), connected, playback, id],
    )
    .ok();
}

/// Save battery levels captured at connection time.
pub fn set_connect_battery(id: i64, left: Option<u8>, right: Option<u8>, case: Option<u8>) {
    let db = conn().lock();
    db.execute(
        "UPDATE sessions SET bat_left_connect=?1, bat_right_connect=?2, bat_case_connect=?3 WHERE id=?4",
        params![left.map(|v| v as i64), right.map(|v| v as i64), case.map(|v| v as i64), id],
    ).ok();
}

/// Save battery levels captured at disconnection time.
pub fn set_disconnect_battery(id: i64, left: Option<u8>, right: Option<u8>, case: Option<u8>) {
    let db = conn().lock();
    db.execute(
        "UPDATE sessions SET bat_left_disc=?1, bat_right_disc=?2, bat_case_disc=?3 WHERE id=?4",
        params![left.map(|v| v as i64), right.map(|v| v as i64), case.map(|v| v as i64), id],
    ).ok();
}

/// Save firmware version string.
pub fn set_firmware(id: i64, firmware: &str) {
    let db = conn().lock();
    db.execute(
        "UPDATE sessions SET firmware=?1 WHERE id=?2",
        params![firmware, id],
    ).ok();
}

pub fn update_session_live(id: i64, connected: f64, playback: f64) {
    let db = conn().lock();
    db.execute(
        "UPDATE sessions SET connected_secs=?1, playback_secs=?2 WHERE id=?3",
        params![connected, playback, id],
    )
    .ok();
}

// ── Daily stats ─────────────────────────────────────────────────────────────

pub fn add_to_daily(day: &NaiveDate, connected: f64, playback: f64) {
    let db = conn().lock();
    db.execute(
        "INSERT INTO daily_stats (day, connected_secs, playback_secs) VALUES (?1,?2,?3)
         ON CONFLICT(day) DO UPDATE SET
             connected_secs = connected_secs + excluded.connected_secs,
             playback_secs  = playback_secs  + excluded.playback_secs",
        params![day.format("%Y-%m-%d").to_string(), connected, playback],
    )
    .ok();
}

// ── Query helpers ────────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone, Default)]
pub struct StatsPeriod {
    pub connected: f64,
    pub playback: f64,
}

pub fn get_stats_for_range(start: &NaiveDate, end: &NaiveDate) -> StatsPeriod {
    let db = conn().lock();
    db.query_row(
        "SELECT COALESCE(SUM(connected_secs),0), COALESCE(SUM(playback_secs),0)
         FROM daily_stats WHERE day >= ?1 AND day <= ?2",
        params![start.format("%Y-%m-%d").to_string(), end.format("%Y-%m-%d").to_string()],
        |row| Ok(StatsPeriod { connected: row.get(0)?, playback: row.get(1)? }),
    )
    .unwrap_or_default()
}

pub fn get_lifetime_stats() -> StatsPeriod {
    let db = conn().lock();
    db.query_row(
        "SELECT COALESCE(SUM(connected_secs),0), COALESCE(SUM(playback_secs),0)
         FROM daily_stats",
        [],
        |row| Ok(StatsPeriod { connected: row.get(0)?, playback: row.get(1)? }),
    )
    .unwrap_or_default()
}

#[derive(serde::Serialize)]
pub struct SessionRow {
    pub id: i64,
    pub session_start: String,
    pub session_end: String,
    pub connected_secs: f64,
    pub playback_secs: f64,
    pub bat_left_connect:  Option<i64>,
    pub bat_right_connect: Option<i64>,
    pub bat_case_connect:  Option<i64>,
    pub bat_left_disc:     Option<i64>,
    pub bat_right_disc:    Option<i64>,
    pub bat_case_disc:     Option<i64>,
    pub firmware:          Option<String>,
    pub interrupted:       i64,
}

pub fn get_recent_sessions(limit: usize) -> Vec<SessionRow> {
    let db = conn().lock();
    let mut stmt = db
        .prepare(
            "SELECT id, session_start, COALESCE(session_end,''), connected_secs, playback_secs,
                    bat_left_connect, bat_right_connect, bat_case_connect,
                    bat_left_disc, bat_right_disc, bat_case_disc, firmware, interrupted
             FROM sessions ORDER BY id DESC LIMIT ?1",
        )
        .expect("prepare failed");
    stmt.query_map(params![limit as i64], |row| {
        Ok(SessionRow {
            id: row.get(0)?,
            session_start: row.get(1)?,
            session_end: row.get(2)?,
            connected_secs: row.get(3)?,
            playback_secs: row.get(4)?,
            bat_left_connect:  row.get(5)?,
            bat_right_connect: row.get(6)?,
            bat_case_connect:  row.get(7)?,
            bat_left_disc:     row.get(8)?,
            bat_right_disc:    row.get(9)?,
            bat_case_disc:     row.get(10)?,
            firmware:          row.get(11)?,
            interrupted:       row.get(12)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

#[derive(serde::Serialize, Clone)]
pub struct DailyStatsRow {
    pub day: String,
    pub connected_secs: f64,
    pub playback_secs: f64,
}

pub fn get_daily_history(limit: usize) -> Vec<DailyStatsRow> {
    let db = conn().lock();
    let mut stmt = db
        .prepare("SELECT day, connected_secs, playback_secs FROM daily_stats ORDER BY day DESC LIMIT ?1")
        .expect("prepare failed");
    stmt.query_map(params![limit as i64], |row| {
        Ok(DailyStatsRow {
            day: row.get(0)?,
            connected_secs: row.get(1)?,
            playback_secs: row.get(2)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

pub fn reset_all_data() {
    let db = conn().lock();
    db.execute_batch(
        "DELETE FROM sessions; DELETE FROM daily_stats; DELETE FROM app_audio_events;",
    )
    .ok();
}

// ── App Audio Events ─────────────────────────────────────────────────────────

/// Opens a new app audio event row and returns its DB id.
pub fn open_app_event(session_id: i64, process_name: &str) -> i64 {
    let now = Local::now().naive_local();
    let db = conn().lock();
    db.execute(
        "INSERT INTO app_audio_events (session_id, process_name, start_ts) VALUES (?1, ?2, ?3)",
        params![
            session_id,
            process_name,
            now.format("%Y-%m-%dT%H:%M:%S").to_string()
        ],
    )
    .ok();
    db.last_insert_rowid()
}

/// Closes an open app audio event row by its id.
pub fn close_app_event(event_id: i64, process_name: &str) {
    let now = Local::now().naive_local();
    let db = conn().lock();
    // Calculate duration from start_ts
    let _ = db.execute(
        "UPDATE app_audio_events
         SET end_ts = ?1,
             duration_secs = (
                 CAST((julianday(?1) - julianday(start_ts)) * 86400.0 AS REAL)
             )
         WHERE id = ?2",
        params![now.format("%Y-%m-%dT%H:%M:%S").to_string(), event_id],
    );
    let _ = process_name; // used by caller for logging only
}

#[derive(serde::Serialize, Clone)]
pub struct AppAudioEventRow {
    pub id: i64,
    pub session_id: i64,
    pub process_name: String,
    pub start_ts: String,
    pub end_ts: Option<String>,
    pub duration_secs: Option<f64>,
}

pub fn get_app_events_for_session(session_id: i64) -> Vec<AppAudioEventRow> {
    let db = conn().lock();
    let sess_end: Option<String> = db
        .query_row(
            "SELECT session_end FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .unwrap_or(None);
    let cutoff = sess_end.filter(|s| !s.is_empty()).unwrap_or_else(|| {
        Local::now().naive_local().format("%Y-%m-%dT%H:%M:%S").to_string()
    });

    let mut stmt = db
        .prepare(
            "SELECT id, session_id, process_name, start_ts, end_ts,
                    COALESCE(duration_secs, CAST((julianday(?2) - julianday(start_ts)) * 86400.0 AS REAL))
             FROM app_audio_events
             WHERE session_id = ?1
             ORDER BY start_ts ASC",
        )
        .expect("prepare failed");
    stmt.query_map(params![session_id, cutoff], |row| {
        Ok(AppAudioEventRow {
            id: row.get(0)?,
            session_id: row.get(1)?,
            process_name: row.get(2)?,
            start_ts: row.get(3)?,
            end_ts: row.get(4)?,
            duration_secs: row.get(5)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

/// Per-app aggregated totals for one session.
#[derive(serde::Serialize, Clone)]
pub struct AppTotal {
    pub process_name: String,
    pub total_secs: f64,
    pub event_count: i64,
}

pub fn get_app_totals_for_session(session_id: i64) -> Vec<AppTotal> {
    let db = conn().lock();
    let sess_end: Option<String> = db
        .query_row(
            "SELECT session_end FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .unwrap_or(None);
    let cutoff = sess_end.filter(|s| !s.is_empty()).unwrap_or_else(|| {
        Local::now().naive_local().format("%Y-%m-%dT%H:%M:%S").to_string()
    });

    let mut stmt = db
        .prepare(
            "SELECT process_name,
                    COALESCE(SUM(
                        COALESCE(duration_secs, CAST((julianday(?2) - julianday(start_ts)) * 86400.0 AS REAL))
                    ), 0) AS total_secs,
                    COUNT(*) AS event_count
             FROM app_audio_events
             WHERE session_id = ?1
             GROUP BY process_name
             ORDER BY total_secs DESC",
        )
        .expect("prepare failed");
    stmt.query_map(params![session_id, cutoff], |row| {
        Ok(AppTotal {
            process_name: row.get(0)?,
            total_secs: row.get(1)?,
            event_count: row.get(2)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

// ── Session Notes ────────────────────────────────────────────────────────────

pub fn set_session_note(session_id: i64, note: &str) {
    let db = conn().lock();
    db.execute(
        "UPDATE sessions SET notes = ?1 WHERE id = ?2",
        params![note, session_id],
    )
    .ok();
}

// ── Extended session row (includes notes) ────────────────────────────────────

#[derive(serde::Serialize, Clone)]
pub struct SessionBreakdownRow {
    pub id: i64,
    pub session_start: String,
    pub session_end: String,
    pub connected_secs: f64,
    pub playback_secs: f64,
    pub bat_left_connect:  Option<i64>,
    pub bat_right_connect: Option<i64>,
    pub bat_case_connect:  Option<i64>,
    pub bat_left_disc:     Option<i64>,
    pub bat_right_disc:    Option<i64>,
    pub bat_case_disc:     Option<i64>,
    pub notes: Option<String>,
    pub interrupted:       i64,
}

pub fn get_sessions_for_breakdown(limit: usize) -> Vec<SessionBreakdownRow> {
    let db = conn().lock();
    let mut stmt = db
        .prepare(
            "SELECT id, session_start, COALESCE(session_end,''), connected_secs, playback_secs,
                    bat_left_connect, bat_right_connect, bat_case_connect,
                    bat_left_disc, bat_right_disc, bat_case_disc, notes, interrupted
             FROM sessions ORDER BY id DESC LIMIT ?1",
        )
        .expect("prepare failed");
    stmt.query_map(params![limit as i64], |row| {
        Ok(SessionBreakdownRow {
            id: row.get(0)?,
            session_start: row.get(1)?,
            session_end: row.get(2)?,
            connected_secs: row.get(3)?,
            playback_secs: row.get(4)?,
            bat_left_connect:  row.get(5)?,
            bat_right_connect: row.get(6)?,
            bat_case_connect:  row.get(7)?,
            bat_left_disc:     row.get(8)?,
            bat_right_disc:    row.get(9)?,
            bat_case_disc:     row.get(10)?,
            notes:             row.get(11)?,
            interrupted:       row.get(12)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

/// Full breakdown payload for a single session.
#[derive(serde::Serialize)]
pub struct SessionBreakdown {
    pub session: SessionBreakdownRow,
    pub app_events: Vec<AppAudioEventRow>,
    pub app_totals: Vec<AppTotal>,
}

pub fn get_session_breakdown(session_id: i64) -> Option<SessionBreakdown> {
    let sessions = get_sessions_for_breakdown(1000);
    let session = sessions.into_iter().find(|s| s.id == session_id)?;
    let app_events = get_app_events_for_session(session_id);
    let app_totals = get_app_totals_for_session(session_id);
    Some(SessionBreakdown { session, app_events, app_totals })
}

// ── Battery Graph Data ────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
pub struct BatteryGraphPoint {
    pub label:         String,
    pub ts:            String,
    pub left_start:    Option<i64>,
    pub left_end:      Option<i64>,
    pub right_start:   Option<i64>,
    pub right_end:     Option<i64>,
    pub case_start:    Option<i64>,
    pub case_end:      Option<i64>,
    pub duration_mins: f64,
}

fn fmt_session_label(ts: &str) -> String {
    if ts.len() >= 16 {
        let mut it = ts.splitn(2, 'T');
        if let (Some(date), Some(time)) = (it.next(), it.next()) {
            let dp: Vec<&str> = date.split('-').collect();
            if dp.len() == 3 {
                let mon: usize = dp[1].parse().unwrap_or(1);
                let day: u32   = dp[2].parse().unwrap_or(1);
                let months = ["Jan","Feb","Mar","Apr","May","Jun",
                              "Jul","Aug","Sep","Oct","Nov","Dec"];
                let m = months[mon.saturating_sub(1).min(11)];
                let t = &time[..time.len().min(5)];
                return format!("{} {} {}", m, day, t);
            }
        }
    }
    ts.to_string()
}

pub fn get_battery_graph_data(duration: &str) -> Vec<BatteryGraphPoint> {
    let db  = conn().lock();
    let sql = match duration {
        "session" =>
            "SELECT session_start, COALESCE(session_end,''),
                    bat_left_connect, bat_left_disc,
                    bat_right_connect, bat_right_disc,
                    bat_case_connect, bat_case_disc,
                    CAST((julianday(COALESCE(NULLIF(session_end,''),datetime('now')))
                          - julianday(session_start)) * 1440.0 AS REAL)
             FROM sessions ORDER BY id DESC LIMIT 1",
        "day" =>
            "SELECT session_start, COALESCE(session_end,''),
                    bat_left_connect, bat_left_disc,
                    bat_right_connect, bat_right_disc,
                    bat_case_connect, bat_case_disc,
                    CAST((julianday(COALESCE(NULLIF(session_end,''),datetime('now')))
                          - julianday(session_start)) * 1440.0 AS REAL)
             FROM sessions
             WHERE date(session_start) = date('now','localtime')
             ORDER BY id ASC",
        "week" =>
            "SELECT session_start, COALESCE(session_end,''),
                    bat_left_connect, bat_left_disc,
                    bat_right_connect, bat_right_disc,
                    bat_case_connect, bat_case_disc,
                    CAST((julianday(COALESCE(NULLIF(session_end,''),datetime('now')))
                          - julianday(session_start)) * 1440.0 AS REAL)
             FROM sessions
             WHERE session_start >= datetime('now','-7 days')
             ORDER BY id ASC",
        _ => // month (default)
            "SELECT session_start, COALESCE(session_end,''),
                    bat_left_connect, bat_left_disc,
                    bat_right_connect, bat_right_disc,
                    bat_case_connect, bat_case_disc,
                    CAST((julianday(COALESCE(NULLIF(session_end,''),datetime('now')))
                          - julianday(session_start)) * 1440.0 AS REAL)
             FROM sessions
             WHERE session_start >= datetime('now','-30 days')
             ORDER BY id ASC",
    };

    let mut stmt = match db.prepare(sql) {
        Ok(s)  => s,
        Err(_) => return vec![],
    };
    stmt.query_map([], |row| {
        let ts: String = row.get(0)?;
        let label = fmt_session_label(&ts);
        Ok(BatteryGraphPoint {
            label,
            ts,
            left_start:    row.get(2)?,
            left_end:      row.get(3)?,
            right_start:   row.get(4)?,
            right_end:     row.get(5)?,
            case_start:    row.get(6)?,
            case_end:      row.get(7)?,
            duration_mins: row.get::<_, Option<f64>>(8)?.unwrap_or(0.0),
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

pub fn cleanup_unclosed_sessions() {
    let db = conn().lock();
    let mut stmt = db.prepare(
        "SELECT id, session_start, connected_secs, playback_secs FROM sessions WHERE session_end IS NULL OR session_end = ''"
    ).expect("Failed to prepare cleanup query");
    
    struct Unclosed {
        id: i64,
        start_str: String,
        connected_secs: f64,
        playback_secs: f64,
    }

    let rows: Vec<Unclosed> = stmt.query_map([], |row| {
        Ok(Unclosed {
            id: row.get(0)?,
            start_str: row.get(1)?,
            connected_secs: row.get(2)?,
            playback_secs: row.get(3)?,
        })
    }).unwrap().filter_map(|r| r.ok()).collect();

    drop(stmt);

    for s in rows {
        let start_dt = match NaiveDateTime::parse_from_str(&s.start_str, "%Y-%m-%dT%H:%M:%S") {
            Ok(dt) => dt,
            Err(_) => continue,
        };
        let end_dt = start_dt + chrono::Duration::seconds(s.connected_secs as i64);
        let end_str = end_dt.format("%Y-%m-%dT%H:%M:%S").to_string();

        db.execute(
            "UPDATE sessions SET session_end = ?1, interrupted = 1 WHERE id = ?2",
            params![end_str, s.id]
        ).ok();

        let day = start_dt.date();
        db.execute(
            "INSERT INTO daily_stats (day, connected_secs, playback_secs) VALUES (?1,?2,?3)
             ON CONFLICT(day) DO UPDATE SET
                 connected_secs = connected_secs + excluded.connected_secs,
                 playback_secs  = playback_secs  + excluded.playback_secs",
            params![day.format("%Y-%m-%d").to_string(), s.connected_secs, s.playback_secs],
        ).ok();
        
        let cutoff = end_str.clone();
        db.execute(
            "UPDATE app_audio_events
             SET end_ts = ?1,
                 duration_secs = (
                     CAST((julianday(?1) - julianday(start_ts)) * 86400.0 AS REAL)
                 )
             WHERE session_id = ?2 AND end_ts IS NULL",
            params![cutoff, s.id],
        ).ok();

        println!("Cleaned up interrupted session id={} (Conn: {}s, Play: {}s)", s.id, s.connected_secs, s.playback_secs);
    }
}
