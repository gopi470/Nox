# Nox

A hybrid background tracking utility and desktop dashboard for monitoring Bluetooth earbuds connection time, active media playback duration, and battery levels on Windows. 

Designed for **Nothing** and **CMF** earbuds, with development and testing primarily focused on the **CMF Buds 2a**. Built using **Rust**, **Tauri (v2)**, and **SQLite**.

---

## Installation

Download the latest Windows installer from:

- [Latest GitHub Release](https://github.com/gopi470/Nox/releases/latest)

On the release page, look in the **Assets** section for the `.msi` or `.exe` installer.

---
## Note

> Nox is currently distributed as an unsigned Windows application.
>
> Because it interacts with Bluetooth devices, monitors audio sessions, stores usage statistics, and can optionally start automatically with Windows, Microsoft Defender SmartScreen or antivirus products may display warnings or machine-learning-based detections on newly released builds.
>
> These detections can occur with low-distribution or unsigned applications and do not automatically indicate malicious behavior. Always verify downloads originate from the official GitHub Releases page.
>
> For transparency, the complete source code is publicly available in this repository and can be reviewed or built from source.
>
> Future releases may include code signing to improve trust and reduce SmartScreen and antivirus warnings.
>
---

## Key Features

- **Real-Time Telemetry**: Queries Left/Right earbud and Charging Case battery percentages and charging states using a native WinRT RFCOMM socket SPP implementation.
- **Background Tracking**: Silently monitors connection presence (PnP query) and media playback (WASAPI session peak meters) with a 5-second silence grace filter.
- **Audio App Attribution**: Logs and details specific application audio usage (e.g., Spotify, Chrome) during sessions.
- **Interactive Analytics**: 
  - Dynamic daily usage rings, active equalizer animations, and consecutive active listening streak calculations.
  - Interactive battery drain line charts with normalized grids, hover guidelines, and date-aware bin-packing pagination.
- **Session Breakdown Directory**: Scrollable connection history with note editing, detailed app audio usage breakdown, and CSV/JSON exporting.
- **System Automation & Utilities**:
  - **Autopause**: Automatically pauses system media playback when earbuds disconnect.
  - **Single Instance**: Singleton instance locking to prevent duplicate tray processes.
  - **Auto-Backup**: Schedules automatic JSON database backups to local storage and Downloads.
  - **Secure Purging**: Protects database resets using Windows user password authentication.


---



## Architecture Overview

Nox operates as a lightweight, dual-process desktop utility designed to run continuously and silently in the background:

```
                   ┌──────────────────────────────────┐
                   │        Tauri Rust Backend        │
                   │      (earbuds-tracker.exe)       │
                   └──────┬────────────────────▲──────┘
                          │                    │
            Tauri Events  │                    │ Tauri Commands
            (IPC push)    │                    │ (IPC request)
                          ▼                    │
                   ┌───────────────────────────┴──────┐
                   │       HTML5 / CSS / JS UI        │
                   │         (Tauri WebView2)         │
                   └──────────────────────────────────┘
```

### 1. **Tauri Backend (Rust)**
Runs silently as a background service:
- **Presence & Connection Monitoring**: Tracks the connection/disconnection state of paired Bluetooth devices by querying Windows PnP devices.
- **Audio Session Tracker**: Monitored via Windows WASAPI session peak meters to measure active playback time with a 5-second silence grace filter.
- **Battery Polling Service**: Runs a background polling thread that opens a WinRT RFCOMM socket connection to the custom Bluetooth SPP service UUID, queries battery statuses, and caches the results.
- **Database Engine**: Embeds an SQLite database to record session telemetry, process-specific audio usage, and daily logs.
- **Auto-Backup Engine**: Automatically schedules database exports to JSON files inside `exports/` and the user's `Downloads/` directory.

### 2. **Frontend Dashboard (HTML/CSS/JS)**
A modern dark-themed WebView2 interface accessed from the system tray:
- **Dynamic Renderers**: Renders daily usage stats using SVG/Canvas dual-ring meters and animated audio equalizers.
- **Battery & Connection Analytics**: Displays historical graphs via ApexCharts.
- **Dynamic Pagination**: Performs date-aware bin-packing calculations in JavaScript to slice large datasets into readable chunks.
- **Local Persistence**: Saves UI configuration preferences inside the browser's `localStorage`.

---

## Project Structure

```
├── test_winrt.ps1             # WinRT Bluetooth prototyping script
├── test.ps1                   # PnP device battery prototyping script
└── earbuds-tracker-tauri/     # Tauri Project root
    ├── src/                   # Frontend assets
    │   ├── index.html         # Core dashboard interface
    │   ├── styles.css         # Styling, themes, animations, layouts
    │   ├── main.js            # Frontend logic, database calls, graph rendering
    │   └── utils.js           # Utility and helper functions
    └── src-tauri/             # Rust source code
        ├── src/
        │   ├── main.rs        # Tauri entrypoint
        │   ├── lib.rs         # Commands and tray setup
        │   ├── app_audio.rs   # Audio process tracking and autopause features
        │   ├── audio.rs       # WASAPI peak audio monitoring
        │   ├── bluetooth.rs   # Audio/PnP connection detection
        │   ├── db.rs          # SQLite migrations and querying
        │   ├── spp.rs         # Serial port protocol for battery stats
        │   └── tracker.rs     # Core session state driver
        └── Cargo.toml         # Rust backend dependencies
```

---

## Developer Installation & Setup

### Prerequisites
1. **Windows 10 / 11**
2. **Node.js** (for Tauri frontend tooling)
3. **Rust and Cargo** toolchain

### Build & Run
1. Clone the repository:
   ```bash
   git clone https://github.com/gopi470/Nox.git
   cd Nox
   ```
2. Navigate to the Tauri project directory, install dependencies, and start development mode:
   ```bash
   cd earbuds-tracker-tauri
   npm install
   npm run tauri dev
   ```
3. To compile a production build:
   ```bash
   npm run tauri build
   ```
   After the build finishes, look in the Tauri output folder for the generated installer or package. The exact path depends on the target format you build, but it is typically under `src-tauri/target/release/bundle/`.

---

## Configuration Details

All application settings, preferences, and logs are stored in the user's local AppData directory: `%APPDATA%\EarbudsTracker\`.

### File Locations

- **Database Path**: All session usage history, audio peaks, and daily stats are persisted to a local SQLite database:
  `%APPDATA%\EarbudsTracker\tracker.db`
- **Application Settings**: Preferences (such as target device name, polling interval, battery step size, autostart, desktop notifications, auto-backup, and autopause settings) are saved in a unified JSON structure:
  `%APPDATA%\EarbudsTracker\settings.json`
- **Auto-Backup State**: The timestamp of the last completed auto-backup run is saved in a sibling configuration file:
  `%APPDATA%\EarbudsTracker\auto_backup_state.json`
### Frontend State

To ensure immediate UI updates, the WebView2 client persists configurations and cache parameters in the browser's `localStorage` (including graph pagination limits, last-known battery levels, daily playback duration cache, daily goals, device name mapping, notification preferences, autostart state, and active UI font sizing).



---

## License & Legal


This project is not affiliated with, sponsored by, or endorsed by **Nothing Technology Limited** or **CMF**. All brand names, logos, and trademarks are the property of their respective owners.
