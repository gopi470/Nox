"""
tracker.py
──────────
Core session tracking logic: wires BluetoothMonitor + AudioMonitor together,
accumulates durations, persists data, and exposes a clean stats API for the UI.
"""

import logging
import threading
import os
import sys
import subprocess
from datetime import datetime, date, timedelta
from typing import Optional

import database as db
from bluetooth_monitor import BluetoothMonitor
from audio_monitor import AudioMonitor

logger = logging.getLogger("tracker")

LIVE_WRITE_INTERVAL = 30   # seconds between crash-safe DB writes


class Tracker:
    """
    Owns connection and playback time accounting.

    State machine:
        IDLE  →  (BT connect)  →  CONNECTED
        CONNECTED  →  (audio play)  →  PLAYING
        PLAYING    →  (audio pause) →  CONNECTED
        CONNECTED  →  (BT disconnect) →  IDLE
    """

    def __init__(self, device_name: str = "CMF Buds 2a"):
        self.device_name = device_name

        # ── Bluetooth monitor ────────────────────────────────────────────────
        self._bt = BluetoothMonitor(
            device_name,
            on_connect=self._on_connect,
            on_disconnect=self._on_disconnect,
        )

        # ── Audio monitor ────────────────────────────────────────────────────
        self._audio = AudioMonitor(
            device_name,
            on_play=self._on_play,
            on_pause=self._on_pause,
        )

        # ── Session state ────────────────────────────────────────────────────
        self._lock              = threading.Lock()
        self._session_id: Optional[int]       = None
        self._session_start: Optional[datetime] = None
        self._play_start: Optional[datetime]    = None

        self._session_connected_secs: float = 0.0
        self._session_playback_secs:  float = 0.0

        # ── Callbacks for UI refresh ─────────────────────────────────────────
        self.on_state_change = None   # callable, set by UI

        # ── Live-write timer ─────────────────────────────────────────────────
        self._live_timer: Optional[threading.Timer] = None

    # ── Lifecycle ─────────────────────────────────────────────────────────────

    def start(self) -> None:
        db.init_db()
        self._audio.start()
        self._bt.start()

    def stop(self) -> None:
        self._cancel_live_timer()
        self._bt.stop()
        self._audio.stop()
        # If still connected at shutdown, close the session
        with self._lock:
            if self._session_id is not None:
                self._finalise_session()

    # ── BT callbacks (called from BluetoothMonitor thread) ───────────────────

    def _on_connect(self) -> None:
        with self._lock:
            now = datetime.now()
            self._session_start            = now
            self._session_connected_secs   = 0.0
            self._session_playback_secs    = 0.0
            self._play_start               = None
            self._session_id               = db.open_session(now)
        self._schedule_live_write()
        logger.info("Session opened id=%s", self._session_id)
        self._notify()
        self._run_event_script("connect")

    def _on_disconnect(self) -> None:
        with self._lock:
            # Stop any ongoing playback segment
            if self._play_start is not None:
                self._session_playback_secs += (
                    datetime.now() - self._play_start
                ).total_seconds()
                self._play_start = None
            self._finalise_session()
        self._cancel_live_timer()
        logger.info("Session closed")
        self._notify()
        self._kill_on_connect_script()
        self._run_event_script("disconnect")

    def _kill_on_connect_script(self) -> None:
        """Finds and terminates any AutoHotkey process running on_connect.ahk."""
        try:
            import psutil
            for proc in psutil.process_iter(['pid', 'name', 'cmdline']):
                try:
                    if proc.info['name'] and "autohotkey" in proc.info['name'].lower():
                        cmdline = proc.info['cmdline']
                        if cmdline and any("on_connect.ahk" in arg.lower() for arg in cmdline):
                            logger.info("Terminating running on_connect script (PID %d)", proc.info['pid'])
                            proc.terminate()
                except (psutil.NoSuchProcess, psutil.AccessDenied, psutil.ZombieProcess):
                    pass
        except Exception as e:
            logger.debug("Failed to scan processes for cleanup: %s", e)

    def _run_event_script(self, event_type: str) -> None:
        """
        Locates and executes user-defined on_connect / on_disconnect scripts.
        Checks the workspace directory and the tracker directory.
        Runs .ahk, .bat, .cmd, .ps1, or .py files.
        """
        current_dir = os.path.dirname(os.path.abspath(__file__))
        parent_dir = os.path.dirname(current_dir)
        
        search_dirs = [parent_dir, current_dir]
        extensions = [".ahk", ".bat", ".cmd", ".ps1", ".py"]
        
        filename_base = f"on_{event_type}"
        
        for directory in search_dirs:
            for ext in extensions:
                script_path = os.path.join(directory, f"{filename_base}{ext}")
                if os.path.exists(script_path):
                    logger.info("Found %s script: %s", event_type, script_path)
                    
                    # If it's an .ahk file, try to run it directly with the AutoHotkey interpreter
                    # to prevent opening it in VS Code or other associated editors.
                    if ext == ".ahk":
                        ahk_paths = [
                            r"C:\Program Files\AutoHotkey\v2\AutoHotkey64.exe",
                            r"C:\Program Files\AutoHotkey\v2\AutoHotkey32.exe",
                            r"C:\Program Files\AutoHotkey\AutoHotkey.exe",
                        ]
                        for ahk_exe in ahk_paths:
                            if os.path.exists(ahk_exe):
                                try:
                                    subprocess.Popen([ahk_exe, script_path], close_fds=True)
                                    return
                                except Exception as e:
                                    logger.error("Failed to run %s with %s: %s", script_path, ahk_exe, e)
                    
                    # For other files, or if AHK interpreter wasn't found, try standard execution
                    try:
                        if ext == ".py":
                            subprocess.Popen([sys.executable, script_path], close_fds=True)
                        elif ext == ".ahk":
                            # Fallback if AHK interpreter wasn't in standard paths
                            os.startfile(script_path)
                        else:
                            subprocess.Popen([script_path], shell=True)
                        return
                    except Exception as e:
                        logger.error("Execution failed for %s: %s", script_path, e)

    # ── Audio callbacks ───────────────────────────────────────────────────────

    def _on_play(self) -> None:
        with self._lock:
            if self._session_id is None:
                return          # audio event but BT not connected – ignore
            if self._play_start is None:
                self._play_start = datetime.now()
        self._notify()

    def _on_pause(self) -> None:
        with self._lock:
            if self._play_start is not None:
                self._session_playback_secs += (
                    datetime.now() - self._play_start
                ).total_seconds()
                self._play_start = None
        self._notify()

    # ── Internal helpers ──────────────────────────────────────────────────────

    def _finalise_session(self) -> None:
        """Must be called under self._lock."""
        if self._session_id is None:
            return

        now     = datetime.now()
        end     = now
        start   = self._session_start
        c_secs  = (end - start).total_seconds()
        p_secs  = self._session_playback_secs

        db.close_session(self._session_id, end, c_secs, p_secs)

        # Write to daily_stats for the day(s) the session spans
        session_date = start.date()
        db.add_to_daily(session_date, c_secs, p_secs)

        self._session_id    = None
        self._session_start = None

    def _schedule_live_write(self) -> None:
        self._cancel_live_timer()
        self._live_timer = threading.Timer(
            LIVE_WRITE_INTERVAL, self._live_write
        )
        self._live_timer.daemon = True
        self._live_timer.start()

    def _cancel_live_timer(self) -> None:
        if self._live_timer is not None:
            self._live_timer.cancel()
            self._live_timer = None

    def _live_write(self) -> None:
        """Persist current partial session; reschedule."""
        with self._lock:
            if self._session_id is None:
                return
            c, p = self._current_durations()
            db.update_session_live(self._session_id, c, p)
        self._schedule_live_write()

    def _current_durations(self) -> tuple[float, float]:
        """Return (connected_secs, playback_secs) for current session snapshot.
        Must be called under self._lock."""
        now = datetime.now()
        c = (
            (now - self._session_start).total_seconds()
            if self._session_start else 0.0
        )
        p = self._session_playback_secs + (
            (now - self._play_start).total_seconds()
            if self._play_start else 0.0
        )
        return c, p

    def _notify(self) -> None:
        if callable(self.on_state_change):
            self.on_state_change()

    # ── Public stats API (called by UI, no lock needed for reads) ─────────────

    def get_snapshot(self) -> dict:
        """
        Returns all the data the UI needs in one call.
        Thread-safe (takes lock briefly).
        """
        with self._lock:
            connected   = self._bt.is_connected()
            playing     = self._audio.is_playing() and connected
            sess_c, sess_p = self._current_durations()

        today      = date.today()
        week_start = today - timedelta(days=today.weekday())
        month_start= today.replace(day=1)

        today_s  = db.get_stats_for_range(today, today)
        week_s   = db.get_stats_for_range(week_start, today)
        month_s  = db.get_stats_for_range(month_start, today)
        life_s   = db.get_lifetime_stats()

        # Add current (not-yet-saved) session to today
        today_s["connected"] += sess_c
        today_s["playback"]  += sess_p
        week_s["connected"]  += sess_c
        week_s["playback"]   += sess_p
        month_s["connected"] += sess_c
        month_s["playback"]  += sess_p
        life_s["connected"]  += sess_c
        life_s["playback"]   += sess_p

        return {
            "connected":   connected,
            "playing":     playing,
            "sess_conn":   sess_c,
            "sess_play":   sess_p,
            "today":       today_s,
            "week":        week_s,
            "month":       month_s,
            "lifetime":    life_s,
        }

    def reset_all(self) -> None:
        db.reset_all_data()
        self._notify()
