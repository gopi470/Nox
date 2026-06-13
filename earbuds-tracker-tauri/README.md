# Nox – Desktop App (Tauri Frontend & Backend)

This folder contains the Tauri (v2) desktop application codebase for **Nox**. It houses the Rust backend core alongside the Vanilla HTML/CSS/JavaScript web dashboard.

---

## Subfolder Structure

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

## Tauri Commands API

The frontend invokes several backend methods defined in `src-tauri/src/lib.rs`:

* **Telemetry & State**: `get_snapshot` (active connection & listening times), `get_device_battery` / `force_query_battery` (read cached or live battery status), `get_active_audio_apps` (processes active in WASAPI peak meter).
* **Session Directory**: `get_sessions` (recent sessions), `get_sessions_for_breakdown` (session list metadata), `get_session_breakdown` (session detail view & app audio totals), `set_session_note` (save user note), `export_session` (serialize session to CSV/JSON).
* **Analytics Data**: `get_daily_history` / `get_daily_history_bounds` (daily usage tracking), `get_battery_graph_data` (battery timeline points), `get_query_log` (underlying event logger).
* **Backup & Migration**: `export_all_data` / `import_all_data` (database JSON import/export), `get_auto_backup_settings` / `set_auto_backup_settings` / `run_auto_backup` (scheduler control).
* **System Settings**: `get_battery_interval` / `set_battery_interval` (polling rate), `get_battery_step` / `set_battery_step` (battery level delta step), `set_device_name` (write target to `settings.json`), `get_paired_devices` (discovery query), `get_startup_enabled` / `set_startup_enabled` (Tauri autostart configuration), `get_autopause_enabled` / `set_autopause_enabled` (pause-on-disconnect control).
* **Authentication & Core**: `verify_windows_password` (Logon authentication for resets), `reset_all` (wipe database), `show_notification` (native toast delivery), `get_app_version` (version info), `is_debug` (runtime flag).
