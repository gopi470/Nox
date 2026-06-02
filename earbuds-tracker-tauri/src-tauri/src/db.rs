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
             );",
        )
        .expect("DB init failed");
        Mutex::new(c)
    })
}

pub fn init_db() {
    let _ = conn(); // trigger OnceCell init
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
}

pub fn get_recent_sessions(limit: usize) -> Vec<SessionRow> {
    let db = conn().lock();
    let mut stmt = db
        .prepare(
            "SELECT id, session_start, COALESCE(session_end,''), connected_secs, playback_secs
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
    db.execute_batch("DELETE FROM sessions; DELETE FROM daily_stats;").ok();
}
