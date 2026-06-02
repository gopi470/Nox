"""
main.py
───────
Application entry-point for EarbudsTracker (Native WinUI 3 edition).

Thread layout
─────────────
  Main thread     → PyQt5 event loop (required by Qt)
  BTMonitor       → daemon thread  (polls WMI/registry)
  AudioMonitor    → daemon thread  (polls WASAPI peak level)
  LiveWrite timer → daemon timer threads (periodic DB flush)

Startup:
  1. Init DB
  2. Start Tracker (spawns BT + Audio monitor threads)
  3. Create PyQt5 Application
  4. Create TrackerWindow (hidden by default)
  5. Create TrayIcon
  6. Enter PyQt5 main loop
"""

import sys
import os
import logging
import threading
from PyQt5.QtWidgets import QApplication

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

# Suppress the harmless pycaw UserWarnings
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
        logger.info("Another instance of EarbudsTracker is already running. Restoring window.")
        try:
            import ctypes
            # Scan for all potential window classes to find the active running window
            for class_name in ["Qt5152QWindowIcon", "Qt5QWindowIcon", "TkTopLevel"]:
                hwnd = ctypes.windll.user32.FindWindowW(class_name, "EarbudsTracker")
                if hwnd:
                    # SW_RESTORE = 9, SW_SHOW = 5
                    ctypes.windll.user32.ShowWindow(hwnd, 9)
                    ctypes.windll.user32.SetForegroundWindow(hwnd)
                    break
        except Exception as e:
            logger.debug("Failed to focus existing window: %s", e)
        sys.exit(0)

    logger.info("EarbudsTracker starting (device=%r)", DEVICE_NAME)

    # ── Core tracker ──────────────────────────────────────────────────────────
    tracker = Tracker(DEVICE_NAME)
    tracker.start()

    # ── PyQt5 App setup ───────────────────────────────────────────────────────
    from PyQt5.QtCore import Qt
    QApplication.setAttribute(Qt.AA_EnableHighDpiScaling, True)
    QApplication.setAttribute(Qt.AA_UseHighDpiPixmaps, True)

    app = QApplication(sys.argv)
    app.setQuitOnLastWindowClosed(False)

    # Create primary UI window
    ui = TrackerWindow(tracker)

    # ── Tray icon ─────────────────────────────────────────────────────────────
    def show_window():
        ui.show_window()

    def quit_app():
        logger.info("Quit requested")
        _shutdown()

    def _shutdown():
        logger.info("Shutting down…")
        tracker.stop()
        tray.hide()
        app.quit()

    tray = TrayIcon(ui, on_open=show_window, on_quit=quit_app, tracker=tracker)
    tray.show()

    logger.info("Running – icon in system tray. Right-click tray icon to quit.")

    # ── PyQt5 event loop (blocking) ───────────────────────────────────────────
    sys.exit(app.exec_())


if __name__ == "__main__":
    main()
