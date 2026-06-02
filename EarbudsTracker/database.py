"""
database.py - SQLite persistence layer for EarbudsTracker
Stores session history and cumulative statistics.
"""

import sqlite3
import os
from datetime import datetime, date, timedelta
from pathlib import Path


DB_PATH = Path(os.getenv("APPDATA")) / "EarbudsTracker" / "tracker.db"


def get_connection() -> sqlite3.Connection:
    DB_PATH.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(str(DB_PATH), detect_types=sqlite3.PARSE_DECLTYPES)
    conn.row_factory = sqlite3.Row
    return conn


def init_db() -> None:
    """Create tables if they don't exist."""
    with get_connection() as conn:
        conn.executescript("""
            CREATE TABLE IF NOT EXISTS sessions (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                session_start   TEXT NOT NULL,
                session_end     TEXT,
                connected_secs  REAL NOT NULL DEFAULT 0,
                playback_secs   REAL NOT NULL DEFAULT 0,
                notes           TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_sessions_start
                ON sessions(session_start);

            CREATE TABLE IF NOT EXISTS daily_stats (
                day             TEXT PRIMARY KEY,   -- ISO date YYYY-MM-DD
                connected_secs  REAL NOT NULL DEFAULT 0,
                playback_secs   REAL NOT NULL DEFAULT 0
            );
        """)


# ── Session management ──────────────────────────────────────────────────────

def open_session(start_time: datetime) -> int:
    """Insert a new open session; return its rowid."""
    with get_connection() as conn:
        cur = conn.execute(
            "INSERT INTO sessions (session_start) VALUES (?)",
            (start_time.isoformat(),),
        )
        return cur.lastrowid


def close_session(
    session_id: int,
    end_time: datetime,
    connected_secs: float,
    playback_secs: float,
) -> None:
    """Finalise a session with its durations."""
    with get_connection() as conn:
        conn.execute(
            """UPDATE sessions
               SET session_end=?, connected_secs=?, playback_secs=?
               WHERE id=?""",
            (end_time.isoformat(), connected_secs, playback_secs, session_id),
        )


def update_session_live(
    session_id: int,
    connected_secs: float,
    playback_secs: float,
) -> None:
    """Periodic live-write so data isn't lost on a crash."""
    with get_connection() as conn:
        conn.execute(
            "UPDATE sessions SET connected_secs=?, playback_secs=? WHERE id=?",
            (connected_secs, playback_secs, session_id),
        )


# ── Daily stats ─────────────────────────────────────────────────────────────

def add_to_daily(day: date, connected_secs: float, playback_secs: float) -> None:
    with get_connection() as conn:
        conn.execute(
            """INSERT INTO daily_stats (day, connected_secs, playback_secs)
               VALUES (?, ?, ?)
               ON CONFLICT(day) DO UPDATE SET
                   connected_secs = connected_secs + excluded.connected_secs,
                   playback_secs  = playback_secs  + excluded.playback_secs""",
            (day.isoformat(), connected_secs, playback_secs),
        )


# ── Query helpers ────────────────────────────────────────────────────────────

def get_stats_for_range(start: date, end: date) -> dict:
    """Sum connected/playback for a date range (inclusive)."""
    with get_connection() as conn:
        row = conn.execute(
            """SELECT COALESCE(SUM(connected_secs),0) AS c,
                      COALESCE(SUM(playback_secs),0)  AS p
               FROM daily_stats
               WHERE day >= ? AND day <= ?""",
            (start.isoformat(), end.isoformat()),
        ).fetchone()
    return {"connected": row["c"], "playback": row["p"]}


def get_lifetime_stats() -> dict:
    with get_connection() as conn:
        row = conn.execute(
            """SELECT COALESCE(SUM(connected_secs),0) AS c,
                      COALESCE(SUM(playback_secs),0)  AS p
               FROM daily_stats"""
        ).fetchone()
    return {"connected": row["c"], "playback": row["p"]}


def get_recent_sessions(limit: int = 50) -> list:
    with get_connection() as conn:
        rows = conn.execute(
            """SELECT id, session_start, session_end,
                      connected_secs, playback_secs
               FROM sessions
               ORDER BY id DESC
               LIMIT ?""",
            (limit,),
        ).fetchall()
    return [dict(r) for r in rows]


def reset_all_data() -> None:
    """Wipe all stored data (user-initiated reset)."""
    with get_connection() as conn:
        conn.executescript("""
            DELETE FROM sessions;
            DELETE FROM daily_stats;
        """)
