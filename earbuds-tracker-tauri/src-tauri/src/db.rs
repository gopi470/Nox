// db.rs – SQLite persistence layer (mirrors database.py)
use chrono::{Duration, Local, NaiveDate, NaiveDateTime};
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

pub fn get_profile_db_path(device_name: &str) -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    let dir = PathBuf::from(appdata).join("EarbudsTracker");
    std::fs::create_dir_all(&dir).ok();

    let name = if device_name.trim().is_empty() { "default" } else { device_name.trim() };
    let sanitized: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();

    dir.join(format!("query_log_{}.db", sanitized))
}

pub fn get_all_profile_db_paths() -> Vec<(String, PathBuf)> {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    let dir = PathBuf::from(appdata).join("EarbudsTracker");
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                if filename.starts_with("query_log_") && filename.ends_with(".db") {
                    let profile_name = &filename[10..filename.len() - 3];
                    files.push((profile_name.to_string(), path));
                }
            }
        }
    }
    files
}

fn conn() -> &'static Mutex<Connection> {
    CONN.get_or_init(|| {
        #[cfg(not(test))]
        let c = Connection::open(db_path()).expect("Cannot open SQLite DB");
        #[cfg(test)]
        let c = Connection::open_in_memory().expect("Cannot open in-memory DB");

        c.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS sessions (
                 id             INTEGER PRIMARY KEY AUTOINCREMENT,
                 session_start  TEXT NOT NULL,
                 session_end    TEXT,
                 connected_secs REAL NOT NULL DEFAULT 0,
                 playback_secs  REAL NOT NULL DEFAULT 0,
                 device_name    TEXT
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
                  ON app_audio_events(session_id);
             CREATE TABLE IF NOT EXISTS query_logs (
                 id             INTEGER PRIMARY KEY AUTOINCREMENT,
                 session_id     INTEGER NOT NULL,
                 session_start  TEXT NOT NULL,
                 event_ts       TEXT NOT NULL,
                 action         TEXT NOT NULL,
                 details        TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_query_logs_session_time
                 ON query_logs(session_id, event_ts DESC);",
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
            "ALTER TABLE sessions ADD COLUMN device_name        TEXT",
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

pub fn open_session(start: &NaiveDateTime, device_name: &str) -> i64 {
    let db = conn().lock();
    db.execute(
        "INSERT INTO sessions (session_start, device_name) VALUES (?1, ?2)",
        params![start.format("%Y-%m-%dT%H:%M:%S").to_string(), device_name],
    )
    .expect("open_session failed");
    db.last_insert_rowid()
}

pub fn get_session_count_for_device(device_name: &str) -> i64 {
    let db = conn().lock();
    db.query_row(
        "SELECT COUNT(*) FROM sessions WHERE device_name = ?1",
        params![device_name],
        |row| row.get(0),
    )
    .unwrap_or(0)
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
        "UPDATE sessions SET
             bat_left_connect  = COALESCE(bat_left_connect,  ?1),
             bat_right_connect = COALESCE(bat_right_connect, ?2),
             bat_case_connect  = COALESCE(bat_case_connect,  ?3)
         WHERE id=?4",
        params![left.map(|v| v as i64), right.map(|v| v as i64), case.map(|v| v as i64), id],
    ).ok();
}

/// Save battery levels captured at disconnection time.
pub fn set_disconnect_battery(id: i64, left: Option<u8>, right: Option<u8>, case: Option<u8>) {
    let db = conn().lock();
    db.execute(
        "UPDATE sessions SET
             bat_left_disc  = COALESCE(?1, bat_left_connect,  bat_left_disc),
             bat_right_disc = COALESCE(?2, bat_right_connect, bat_right_disc),
             bat_case_disc  = COALESCE(?3, bat_case_connect,  bat_case_disc)
         WHERE id=?4",
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

pub fn insert_query_log(
    device_name: &str,
    session_id: i64,
    session_start: &str,
    event_ts: &str,
    action: &str,
    details: &str,
) {
    let db_path = get_profile_db_path(device_name);
    if let Ok(db) = Connection::open(&db_path) {
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS query_logs (
                 id             INTEGER PRIMARY KEY AUTOINCREMENT,
                 session_id     INTEGER NOT NULL,
                 session_start  TEXT NOT NULL,
                 event_ts       TEXT NOT NULL,
                 action         TEXT NOT NULL,
                 details        TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_query_logs_session_time
                 ON query_logs(session_id, event_ts DESC);"
        ).ok();

        db.execute(
            "INSERT INTO query_logs (session_id, session_start, event_ts, action, details)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, session_start, event_ts, action, details],
        ).ok();
    }
}

// ── Query helpers ────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct StatsPeriod {
    pub connected: f64,
    pub playback: f64,
}

pub fn get_stats_for_range(device_name: &str, start: &NaiveDate, end: &NaiveDate) -> StatsPeriod {
    let db = conn().lock();
    db.query_row(
        "SELECT COALESCE(SUM(connected_secs),0), COALESCE(SUM(playback_secs),0)
         FROM sessions
         WHERE substr(session_start, 1, 10) >= ?1 AND substr(session_start, 1, 10) <= ?2
           AND device_name = ?3",
        params![
            start.format("%Y-%m-%d").to_string(),
            end.format("%Y-%m-%d").to_string(),
            device_name
        ],
        |row| Ok(StatsPeriod { connected: row.get(0)?, playback: row.get(1)? }),
    )
    .unwrap_or_default()
}

pub fn get_lifetime_stats(device_name: &str) -> StatsPeriod {
    let db = conn().lock();
    db.query_row(
        "SELECT COALESCE(SUM(connected_secs),0), COALESCE(SUM(playback_secs),0)
         FROM sessions WHERE device_name = ?1",
        params![device_name],
        |row| Ok(StatsPeriod { connected: row.get(0)?, playback: row.get(1)? }),
    )
    .unwrap_or_default()
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct QueryLogRow {
    pub id: i64,
    pub session_id: i64,
    pub session_start: String,
    pub event_ts: String,
    pub action: String,
    pub details: String,
}

pub fn get_query_log(device_name: &str, limit: usize) -> Vec<QueryLogRow> {
    let db_path = get_profile_db_path(device_name);
    let db = match Connection::open(&db_path) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS query_logs (
             id             INTEGER PRIMARY KEY AUTOINCREMENT,
             session_id     INTEGER NOT NULL,
             session_start  TEXT NOT NULL,
             event_ts       TEXT NOT NULL,
             action         TEXT NOT NULL,
             details        TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_query_logs_session_time
             ON query_logs(session_id, event_ts DESC);"
    ).ok();

    let mut stmt = match db.prepare(
        "SELECT id, session_id, session_start, event_ts, action, details
         FROM query_logs
         ORDER BY id DESC
         LIMIT ?1"
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    stmt.query_map(params![limit as i64], |row| {
        Ok(QueryLogRow {
            id: row.get(0)?,
            session_id: row.get(1)?,
            session_start: row.get(2)?,
            event_ts: row.get(3)?,
            action: row.get(4)?,
            details: row.get(5)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct AppAudioEventBackupRow {
    pub id: i64,
    pub session_id: i64,
    pub process_name: String,
    pub start_ts: String,
    pub end_ts: Option<String>,
    pub duration_secs: Option<f64>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct BackupData {
    pub sessions: Vec<SessionBreakdownRow>,
    pub daily_stats: Vec<DailyStatsRow>,
    pub app_audio_events: Vec<AppAudioEventBackupRow>,
    pub query_logs: Vec<QueryLogRow>,
}

pub fn get_all_sessions() -> Vec<SessionBreakdownRow> {
    let db = conn().lock();
    let mut stmt = db
        .prepare(
            "SELECT id, session_start, COALESCE(session_end,''), connected_secs, playback_secs,
                    bat_left_connect, bat_right_connect, bat_case_connect,
                    bat_left_disc, bat_right_disc, bat_case_disc, notes, interrupted, device_name
             FROM sessions ORDER BY id ASC",
        )
        .expect("prepare failed");
    stmt.query_map([], |row| {
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
            device_name:       row.get(13)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

pub fn get_all_daily_stats() -> Vec<DailyStatsRow> {
    let db = conn().lock();
    let mut stmt = db
        .prepare("SELECT day, connected_secs, playback_secs FROM daily_stats ORDER BY day ASC")
        .expect("prepare failed");
    stmt.query_map([], |row| {
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

pub fn get_all_app_audio_events() -> Vec<AppAudioEventBackupRow> {
    let db = conn().lock();
    let mut stmt = db
        .prepare(
            "SELECT id, session_id, process_name, start_ts, end_ts, duration_secs
             FROM app_audio_events
             ORDER BY id ASC",
        )
        .expect("prepare failed");
    stmt.query_map([], |row| {
        Ok(AppAudioEventBackupRow {
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

pub fn get_all_query_logs() -> Vec<QueryLogRow> {
    let mut all_logs = Vec::new();
    
    // 1. Read from profile-specific DB files
    for (_, path) in get_all_profile_db_paths() {
        if let Ok(db) = Connection::open(&path) {
            if let Ok(mut stmt) = db.prepare(
                "SELECT id, session_id, session_start, event_ts, action, details
                 FROM query_logs
                 ORDER BY id ASC"
            ) {
                if let Ok(rows) = stmt.query_map([], |row| {
                    Ok(QueryLogRow {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        session_start: row.get(2)?,
                        event_ts: row.get(3)?,
                        action: row.get(4)?,
                        details: row.get(5)?,
                    })
                }) {
                    all_logs.extend(rows.filter_map(|r| r.ok()));
                }
            }
        }
    }
    
    // 2. Also read legacy logs from the main DB if any exist
    let main_db = conn().lock();
    if let Ok(mut stmt) = main_db.prepare(
        "SELECT id, session_id, session_start, event_ts, action, details
         FROM query_logs
         ORDER BY id ASC"
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(QueryLogRow {
                id: row.get(0)?,
                session_id: row.get(1)?,
                session_start: row.get(2)?,
                event_ts: row.get(3)?,
                action: row.get(4)?,
                details: row.get(5)?,
            })
        }) {
            all_logs.extend(rows.filter_map(|r| r.ok()));
        }
    }
    
    all_logs
}

pub fn export_backup() -> BackupData {
    BackupData {
        sessions: get_all_sessions(),
        daily_stats: get_all_daily_stats(),
        app_audio_events: get_all_app_audio_events(),
        query_logs: get_all_query_logs(),
    }
}

pub fn import_backup(backup: &BackupData, default_device_name: &str) {
    // 1. Delete all existing profile-specific query log DB files
    for (_, path) in get_all_profile_db_paths() {
        let _ = std::fs::remove_file(path);
    }

    let mut db = conn().lock();
    let tx = db.transaction().expect("begin import transaction failed");

    tx.execute_batch(
        "DELETE FROM sessions;
         DELETE FROM daily_stats;
         DELETE FROM app_audio_events;
         DELETE FROM query_logs;
         DELETE FROM sqlite_sequence WHERE name IN ('sessions', 'app_audio_events', 'query_logs');",
    )
    .ok();

    for row in &backup.sessions {
        let dev_name = row.device_name.as_deref().unwrap_or(default_device_name);
        let dev_name = if dev_name.is_empty() { default_device_name } else { dev_name };
        tx.execute(
            "INSERT INTO sessions (
                id, session_start, session_end, connected_secs, playback_secs,
                bat_left_connect, bat_right_connect, bat_case_connect,
                bat_left_disc, bat_right_disc, bat_case_disc, notes, interrupted, device_name
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                row.id,
                row.session_start,
                row.session_end,
                row.connected_secs,
                row.playback_secs,
                row.bat_left_connect,
                row.bat_right_connect,
                row.bat_case_connect,
                row.bat_left_disc,
                row.bat_right_disc,
                row.bat_case_disc,
                row.notes,
                row.interrupted,
                dev_name,
            ],
        )
        .expect("failed to import session row");
    }

    for row in &backup.daily_stats {
        tx.execute(
            "INSERT INTO daily_stats (day, connected_secs, playback_secs) VALUES (?1, ?2, ?3)",
            params![row.day, row.connected_secs, row.playback_secs],
        )
        .expect("failed to import daily row");
    }

    for row in &backup.app_audio_events {
        tx.execute(
            "INSERT INTO app_audio_events (id, session_id, process_name, start_ts, end_ts, duration_secs)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                row.id,
                row.session_id,
                row.process_name,
                row.start_ts,
                row.end_ts,
                row.duration_secs,
            ],
        )
        .expect("failed to import app audio row");
    }

    // Import query logs to their profile-specific databases
    for row in &backup.query_logs {
        let dev_name = backup.sessions.iter()
            .find(|s| s.id == row.session_id)
            .and_then(|s| s.device_name.as_deref())
            .unwrap_or(default_device_name);
        let dev_name = if dev_name.is_empty() { default_device_name } else { dev_name };

        let db_path = get_profile_db_path(dev_name);
        if let Ok(db_conn) = Connection::open(&db_path) {
            db_conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS query_logs (
                     id             INTEGER PRIMARY KEY AUTOINCREMENT,
                     session_id     INTEGER NOT NULL,
                     session_start  TEXT NOT NULL,
                     event_ts       TEXT NOT NULL,
                     action         TEXT NOT NULL,
                     details        TEXT NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS idx_query_logs_session_time
                     ON query_logs(session_id, event_ts DESC);"
            ).ok();

            db_conn.execute(
                "INSERT INTO query_logs (id, session_id, session_start, event_ts, action, details)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    row.id,
                    row.session_id,
                    row.session_start,
                    row.event_ts,
                    row.action,
                    row.details,
                ],
            ).ok();
        }
    }

    tx.commit().expect("commit import transaction failed");
}

#[derive(serde::Serialize, serde::Deserialize)]
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
    #[serde(default)]
    pub device_name: Option<String>,
}

pub fn get_recent_sessions(device_name: &str, limit: usize) -> Vec<SessionRow> {
    let db = conn().lock();
    let mut stmt = db
        .prepare(
            "SELECT id, session_start, COALESCE(session_end,''), connected_secs, playback_secs,
                    bat_left_connect, bat_right_connect, bat_case_connect,
                    bat_left_disc, bat_right_disc, bat_case_disc, firmware, interrupted, device_name
             FROM sessions 
             WHERE device_name = ?1
             ORDER BY id DESC LIMIT ?2",
        )
        .expect("prepare failed");
    stmt.query_map(params![device_name, limit as i64], |row| {
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
            device_name:       row.get(13)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct DailyStatsRow {
    pub day: String,
    pub connected_secs: f64,
    pub playback_secs: f64,
}

pub fn get_daily_history(device_name: &str, week_offset: i64) -> Vec<DailyStatsRow> {
    let today = Local::now().date_naive();
    let end = today - Duration::days(week_offset.saturating_mul(7));
    let start = end - Duration::days(6);
    let db = conn().lock();
    let mut stmt = db
        .prepare(
            "SELECT substr(session_start, 1, 10) as day, 
                    COALESCE(SUM(connected_secs), 0.0), 
                    COALESCE(SUM(playback_secs), 0.0)
             FROM sessions
             WHERE device_name = ?1 
               AND substr(session_start, 1, 10) BETWEEN ?2 AND ?3
             GROUP BY day
             ORDER BY day ASC",
        )
        .expect("prepare failed");
    stmt.query_map(
        params![device_name, start.format("%Y-%m-%d").to_string(), end.format("%Y-%m-%d").to_string()],
        |row| {
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

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct DailyHistoryBounds {
    pub oldest_day: Option<String>,
    pub newest_day: Option<String>,
}

pub fn get_daily_history_bounds(device_name: &str) -> DailyHistoryBounds {
    let db = conn().lock();
    let mut stmt = match db.prepare(
        "SELECT MIN(substr(session_start, 1, 10)), MAX(substr(session_start, 1, 10)) 
         FROM sessions 
         WHERE device_name = ?1"
    ) {
        Ok(stmt) => stmt,
        Err(_) => {
            return DailyHistoryBounds {
                oldest_day: None,
                newest_day: None,
            };
        }
    };

    match stmt.query_row(params![device_name], |row| {
        Ok(DailyHistoryBounds {
            oldest_day: row.get(0).ok(),
            newest_day: row.get(1).ok(),
        })
    }) {
        Ok(bounds) => bounds,
        Err(_) => DailyHistoryBounds {
            oldest_day: None,
            newest_day: None,
        },
    }
}

pub fn reset_all_data() {
    let db = conn().lock();
    db.execute_batch(
        "DELETE FROM sessions; DELETE FROM daily_stats; DELETE FROM app_audio_events; DELETE FROM query_logs;
         DELETE FROM sqlite_sequence WHERE name IN ('sessions', 'app_audio_events', 'query_logs');",
    )
    .ok();

    // Delete all profile-specific query log files
    for (_, path) in get_all_profile_db_paths() {
        let _ = std::fs::remove_file(path);
    }
}

pub fn delete_profile_data(device_name: &str) {
    let db = conn().lock();
    // 1. Delete app audio events for matching sessions
    let _ = db.execute(
        "DELETE FROM app_audio_events WHERE session_id IN (SELECT id FROM sessions WHERE device_name = ?1)",
        params![device_name],
    );
    // 2. Delete query logs for matching sessions (legacy)
    let _ = db.execute(
        "DELETE FROM query_logs WHERE session_id IN (SELECT id FROM sessions WHERE device_name = ?1)",
        params![device_name],
    );
    // 3. Delete matching sessions
    let _ = db.execute(
        "DELETE FROM sessions WHERE device_name = ?1",
        params![device_name],
    );

    // 4. Delete the profile-specific query log database file
    let path = get_profile_db_path(device_name);
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
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

#[derive(serde::Serialize, serde::Deserialize, Clone)]
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
    #[serde(default)]
    pub device_name: Option<String>,
}

pub fn get_session_by_id(id: i64) -> Option<SessionBreakdownRow> {
    let db = conn().lock();
    db.query_row(
        "SELECT id, session_start, COALESCE(session_end,''), connected_secs, playback_secs,
                bat_left_connect, bat_right_connect, bat_case_connect,
                bat_left_disc, bat_right_disc, bat_case_disc, notes, interrupted, device_name
         FROM sessions WHERE id = ?1",
        params![id],
        |row| {
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
                device_name:       row.get(13)?,
            })
        },
    )
    .ok()
}

pub fn get_sessions_for_breakdown(device_name: Option<&str>, limit: usize) -> Vec<SessionBreakdownRow> {
    let db = conn().lock();
    if let Some(dev) = device_name {
        let mut stmt = db.prepare(
            "SELECT id, session_start, COALESCE(session_end,''), connected_secs, playback_secs,
                    bat_left_connect, bat_right_connect, bat_case_connect,
                    bat_left_disc, bat_right_disc, bat_case_disc, notes, interrupted, device_name
             FROM sessions 
             WHERE device_name = ?1
             ORDER BY id DESC LIMIT ?2"
        ).expect("prepare failed");
        stmt.query_map(params![dev, limit as i64], |row| {
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
                device_name:       row.get(13)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    } else {
        let mut stmt = db.prepare(
            "SELECT id, session_start, COALESCE(session_end,''), connected_secs, playback_secs,
                    bat_left_connect, bat_right_connect, bat_case_connect,
                    bat_left_disc, bat_right_disc, bat_case_disc, notes, interrupted, device_name
             FROM sessions ORDER BY id DESC LIMIT ?1"
        ).expect("prepare failed");
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
                device_name:       row.get(13)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }
}

/// Full breakdown payload for a single session.
#[derive(serde::Serialize)]
pub struct SessionBreakdown {
    pub session: SessionBreakdownRow,
    pub app_events: Vec<AppAudioEventRow>,
    pub app_totals: Vec<AppTotal>,
}

pub fn get_session_breakdown(session_id: i64) -> Option<SessionBreakdown> {
    let session = get_session_by_id(session_id)?;
    let app_events = get_app_events_for_session(session_id);
    let app_totals = get_app_totals_for_session(session_id);
    Some(SessionBreakdown { session, app_events, app_totals })
}

// ── Battery Graph Data ────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
pub struct BatteryGraphPoint {
    pub label:          String,
    pub ts:             String,
    pub left_start:     Option<i64>,
    pub left_end:       Option<i64>,
    pub right_start:    Option<i64>,
    pub right_end:      Option<i64>,
    pub case_start:     Option<i64>,
    pub case_end:       Option<i64>,
    pub duration_mins:  f64,
    pub session_end:    String,
    pub connected_secs: f64,
    pub playback_secs:  f64,
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

pub fn get_battery_graph_data(device_name: &str, duration: &str) -> Vec<BatteryGraphPoint> {
    let db  = conn().lock();
    let sql = match duration {
        "session" =>
            "SELECT session_start, COALESCE(session_end,''),
                    bat_left_connect, bat_left_disc,
                    bat_right_connect, bat_right_disc,
                    bat_case_connect, bat_case_disc,
                    CAST((julianday(COALESCE(NULLIF(session_end,''),datetime('now')))
                          - julianday(session_start)) * 1440.0 AS REAL),
                    connected_secs,
                    playback_secs
             FROM sessions 
             WHERE device_name = ?1
             ORDER BY id DESC LIMIT 1",
        "day" =>
            "SELECT session_start, COALESCE(session_end,''),
                    bat_left_connect, bat_left_disc,
                    bat_right_connect, bat_right_disc,
                    bat_case_connect, bat_case_disc,
                    CAST((julianday(COALESCE(NULLIF(session_end,''),datetime('now')))
                          - julianday(session_start)) * 1440.0 AS REAL),
                    connected_secs,
                    playback_secs
             FROM sessions
             WHERE device_name = ?1 AND date(session_start) = date('now','localtime')
             ORDER BY id ASC",
        "week" =>
            "SELECT session_start, COALESCE(session_end,''),
                    bat_left_connect, bat_left_disc,
                    bat_right_connect, bat_right_disc,
                    bat_case_connect, bat_case_disc,
                    CAST((julianday(COALESCE(NULLIF(session_end,''),datetime('now')))
                          - julianday(session_start)) * 1440.0 AS REAL),
                    connected_secs,
                    playback_secs
             FROM sessions
             WHERE device_name = ?1 AND session_start >= datetime('now','-7 days')
             ORDER BY id ASC",
        _ => // month (default)
            "SELECT session_start, COALESCE(session_end,''),
                    bat_left_connect, bat_left_disc,
                    bat_right_connect, bat_right_disc,
                    bat_case_connect, bat_case_disc,
                    CAST((julianday(COALESCE(NULLIF(session_end,''),datetime('now')))
                          - julianday(session_start)) * 1440.0 AS REAL),
                    connected_secs,
                    playback_secs
             FROM sessions
             WHERE device_name = ?1 AND session_start >= datetime('now','-30 days')
             ORDER BY id ASC",
    };

    let mut stmt = match db.prepare(sql) {
        Ok(s)  => s,
        Err(_) => return vec![],
    };
    stmt.query_map(params![device_name], |row| {
        let ts: String = row.get(0)?;
        let session_end: String = row.get(1)?;
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
            session_end,
            connected_secs: row.get::<_, Option<f64>>(9)?.unwrap_or(0.0),
            playback_secs:  row.get::<_, Option<f64>>(10)?.unwrap_or(0.0),
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, Duration};
    use parking_lot::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn setup_db() -> parking_lot::MutexGuard<'static, ()> {
        let lock = TEST_LOCK.lock();
        reset_all_data();
        lock
    }

    fn add_test_session(day: &NaiveDate, connected: f64, playback: f64) {
        let db = conn().lock();
        let session_start = day.and_hms_opt(12, 0, 0).unwrap().format("%Y-%m-%d %H:%M:%S").to_string();
        let session_end = day.and_hms_opt(12, 0, 0).unwrap()
            .checked_add_signed(chrono::Duration::seconds(connected as i64))
            .unwrap()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        db.execute(
            "INSERT INTO sessions (
                session_start, session_end, connected_secs, playback_secs,
                bat_left_connect, bat_right_connect, bat_case_connect,
                bat_left_disc, bat_right_disc, bat_case_disc, notes, interrupted, device_name
            ) VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 0, 'TestEarbuds')",
            params![
                session_start,
                session_end,
                connected,
                playback
            ],
        )
        .expect("insert test session failed");
    }

    #[test]
    fn test_get_daily_history_empty() {
        let _lock = setup_db();
        let history = get_daily_history("TestEarbuds", 0);
        assert!(history.is_empty());
    }

    #[test]
    fn test_get_daily_history_current_week() {
        let _lock = setup_db();
        let today = Local::now().date_naive();

        // Add data for today and 2 days ago
        add_test_session(&today, 100.0, 50.0);
        add_test_session(&(today - Duration::days(2)), 200.0, 100.0);

        // Add data for 8 days ago (should be out of range for week_offset 0)
        add_test_session(&(today - Duration::days(8)), 300.0, 150.0);

        let history = get_daily_history("TestEarbuds", 0);

        assert_eq!(history.len(), 2);
        // Ordering in code is ASC
        assert_eq!(history[0].day, (today - Duration::days(2)).format("%Y-%m-%d").to_string());
        assert_eq!(history[0].connected_secs, 200.0);
        assert_eq!(history[1].day, today.format("%Y-%m-%d").to_string());
        assert_eq!(history[1].connected_secs, 100.0);
    }

    #[test]
    fn test_get_daily_history_previous_week() {
        let _lock = setup_db();
        let today = Local::now().date_naive();

        // Data in current week
        add_test_session(&today, 100.0, 50.0);

        // Data in previous week (offset 1: today-7 to today-13)
        let last_week_day = today - Duration::days(10);
        add_test_session(&last_week_day, 500.0, 250.0);

        let history = get_daily_history("TestEarbuds", 1);

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].day, last_week_day.format("%Y-%m-%d").to_string());
        assert_eq!(history[0].connected_secs, 500.0);
    }

    #[test]
    fn test_get_daily_history_ordering() {
        let _lock = setup_db();
        let today = Local::now().date_naive();

        add_test_session(&today, 100.0, 50.0);
        add_test_session(&(today - Duration::days(1)), 200.0, 100.0);
        add_test_session(&(today - Duration::days(2)), 300.0, 150.0);

        let history = get_daily_history("TestEarbuds", 0);

        assert_eq!(history.len(), 3);
        assert!(history[0].day < history[1].day);
        assert!(history[1].day < history[2].day);
    }

    #[test]
    fn test_get_daily_history_boundaries() {
        let _lock = setup_db();
        let today = Local::now().date_naive();

        // week_offset 0 is today to today-6
        let day_6 = today - Duration::days(6);
        let day_7 = today - Duration::days(7);

        add_test_session(&today, 1.0, 1.0);
        add_test_session(&day_6, 6.0, 6.0);
        add_test_session(&day_7, 7.0, 7.0);

        let history = get_daily_history("TestEarbuds", 0);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].connected_secs, 6.0);
        assert_eq!(history[1].connected_secs, 1.0);
    }
}
