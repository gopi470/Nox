# Earbuds Tracker – Desktop App (Tauri Frontend & Backend)

This folder contains the Tauri (v2) desktop application codebase for **Earbuds Tracker**. It houses the Rust backend core alongside the Vanilla HTML/CSS/JavaScript web dashboard.

---

## 📂 Subfolder Structure

- `src/` - **Frontend Dashboard Application**
  - `index.html` - The dashboard core, containing pages (Dashboard, History, Statistics, Session Breakdown, Settings).
  - `styles.css` - Custom styling theme, support for glassmorphism, flexbox layout adjustments, and custom fonts.
  - `main.js` - Dynamic IPC handlers, chart rendering logic, navigation router, and custom-space alignment calculations.
  - `fonts/` - Holds NDot and other bespoke typefaces.
- `src-tauri/` - **Rust Desktop Backend**
  - `src/main.rs` - Tauri process entry point.
  - `src/lib.rs` - Commands registry for communication between Frontend JS and Rust (e.g. settings storage, verifying logon credentials, querying battery).
  - `src/bluetooth.rs` - Device status & connection monitoring.
  - `src/audio.rs` - WASAPI peak loudness metrics for playback measurement.
  - `src/db.rs` - SQLite schema definition, read/write helper queries, and connection logging.
  - `src/spp.rs` - Proprietary SPP RFCOMM sockets to poll Bluetooth battery levels.
  - `src/tracker.rs` - Active session state driver loop.

---

## 🛠️ Tauri Commands API

The frontend invokes several backend methods defined in `src-tauri/src/lib.rs`:

* `get_snapshot` - Fetches state containing connections, active playing timers, and usage statistics.
* `get_sessions` - Retreives a list of recent connected sessions.
* `get_sessions_for_breakdown` - Returns metadata for the session directory.
* `get_session_breakdown` - Retrieves application usage breakdown and battery levels for a specific session.
* `set_session_note` - Writes a custom text note for a session.
* `export_session` - Serializes session records into CSV/JSON format.
* `get_battery_interval` / `set_battery_interval` - Gets or sets the custom battery query interval in seconds.
* `set_device_name` - Sets the target Bluetooth device (saves to `target_device.txt`).
* `get_paired_devices` - Queries Windows PnP for paired Bluetooth devices.
* `verify_windows_password` - Authenticates user password using Windows Logon API for secure database wipes.
* `reset_all` - Drops all table rows in the database to clear data.
