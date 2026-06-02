"""
main.py
───────
Application entry-point for EarbudsTracker.

Thread layout
─────────────
  Main thread     → Tkinter event loop (required by Tk)
  BTMonitor       → daemon thread  (polls WMI/registry)
  AudioMonitor    → daemon thread  (polls WASAPI peak level)
  TrayIcon        → daemon thread  (pystray event loop)
  LiveWrite timer → daemon timer threads (periodic DB flush)

Startup:
  1. Init DB
  2. Start Tracker (spawns BT + Audio monitor threads)
  3. Start TrayIcon (daemon thread)
  4. Create TrackerWindow (hidden by default)
  5. Enter Tk main loop

Shutdown:
  Tray "Quit" → calls _quit() on main thread via Tk.after
              → stops Tracker (finalises open session)
              → stops TrayIcon
              → destroys Tk root
"""

import sys
import os
import logging
import threading
import tkinter as tk

# ── Logging setup ────────────────────────────────────────────────────────────
LOG_DIR = os.path.join(os.getenv("APPDATA"), "EarbudsTracker")
os.makedirs(LOG_DIR, exist_ok=True)

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(name)-16s %(levelname)-8s %(message)s",
    handlers=[
        logging.FileHandler(os.path.join(LOG_DIR, "tracker.log"), encoding="utf-8"),
        logging.StreamHandler(sys.stdout),
    ],
)
logger = logging.getLogger("main")

# Suppress the harmless pycaw "COMError attempting to get property 28/29"
# UserWarnings — these occur when pycaw reads optional BT device properties
# that some endpoints don't support.  They do not affect functionality.
import warnings
warnings.filterwarnings("ignore", category=UserWarning, module="pycaw")

# ── Imports (after logging is configured) ────────────────────────────────────
from tracker import Tracker
from ui import TrackerWindow
from tray import TrayIcon

DEVICE_NAME = "CMF Buds 2a"


_single_instance_mutex = None

def main():
    # Enforce single instance via named mutex on Windows
    global _single_instance_mutex
    import ctypes
    mutex_name = "Global\\EarbudsTrackerSingleInstanceMutex"
    _single_instance_mutex = ctypes.windll.kernel32.CreateMutexW(None, True, mutex_name)
    last_error = ctypes.windll.kernel32.GetLastError()
    
    if last_error == 183:  # ERROR_ALREADY_EXISTS
        logger.info("Another instance of EarbudsTracker is already running. Exiting.")
        ctypes.windll.user32.MessageBoxW(
            0,
            "EarbudsTracker is already running in the system tray.",
            "EarbudsTracker",
            0x40 | 0x0  # MB_ICONINFORMATION | MB_OK
        )
        sys.exit(0)

    logger.info("EarbudsTracker starting (device=%r)", DEVICE_NAME)

    # ── Core tracker ──────────────────────────────────────────────────────────
    tracker = Tracker(DEVICE_NAME)
    tracker.start()

    # ── Tkinter root (hidden until user opens) ───────────────────────────────
    ui = TrackerWindow(tracker, on_quit=lambda: None)   # on_quit patched below
    ui.root.withdraw()   # start hidden in tray

    # ── Tray icon ─────────────────────────────────────────────────────────────
    def show_window():
        ui.root.after(0, ui.show)

    def quit_app():
        logger.info("Quit requested")
        # Schedule shutdown on the Tk thread
        ui.root.after(0, _shutdown)

    def _shutdown():
        logger.info("Shutting down…")
        tracker.stop()
        tray.stop()
        ui.root.destroy()

    tray = TrayIcon(on_open=show_window, on_quit=quit_app, tracker=tracker)
    tray.start()

    logger.info("Running – icon in system tray.  Right-click tray icon to quit.")

    # ── Tk main loop (blocking) ───────────────────────────────────────────────
    ui.run()
    logger.info("Exited cleanly")


if __name__ == "__main__":
    main()
