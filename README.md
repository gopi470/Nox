# Earbuds-Tracker

A hybrid background tracking utility and desktop dashboard for monitoring Bluetooth earbuds connection time, active media playback duration, and battery levels on Windows. 

Designed specifically for the **CMF Buds 2a** (with support for custom-named paired devices) using **Rust**, **Tauri (v2)**, **SQLite**, and **AutoHotkey**.

---

## Key Features

* **Real-time Battery Tracking**: Queries battery percentage (Left earbud, Right earbud, and Charging Case) and charging states using a custom implementation of the Serial Port Profile (SPP) protocol.
  - **Live UI Indicators**: Displays battery levels with animated charging status bars and interactive detail states.
  - **Last Known State Cache**: Automatically persists battery states so that the dashboard shows disconnected earbuds' last-known levels.
* **Active Playback Monitoring**: Polls Windows WASAPI session peak meters to measure the exact time audio is playing through the earbuds, utilizing a 5-second silence grace filter to prevent flicker.
* **Presence Detection**: Utilizes WASAPI and Windows MMDevice PnP presence monitoring to accurately detect Bluetooth connections.
* **Interactive Dashboard**:
  - **Dynamic Dual-Ring Canvas**: Shows daily connection (outer ring) and media playback (inner ring) compared to a daily goal.
  - **Animated Equalizer**: Animates when media is active.
  - **Daily Streaks**: Calculates consecutive active listening days.
  - **7-Day Bar Chart**: Renders daily usage comparison on a custom-drawn canvas chart.
* **Session Breakdown**:
  - **Historical Directory**: Scrollable lists of past sessions formatted with monospaced character columns for perfect alignment.
  - **Usage Detail Panels**: View specific connection stats, app usage duration breakdown, notes editor, and canvas-based battery drain charts.
  - **Export Formats**: Download session summaries and app usage data in CSV or JSON.
* **Configurable Settings**: Custom playback goals, target device name mapping (with automatic paired-device discovery), desktop notification toggles, and adjustable battery polling intervals (from 30 seconds to 30 minutes).
* **Secure Data Purging**: Requires Windows account password authorization (via Windows Logon APIs) before allowing database resets.
* **Resource Efficient**: Auto-runs in the background and automatically shuts down 5 seconds after earbud disconnection (unless the dashboard window is active).


---

## 🏗️ Architecture Overview

The system operates across three primary processes:

```
                  ┌───────────────────────┐
                  │ AutoHotkey Launcher   │
                  │ (BluetoothLauncher)   │
                  └───────────┬───────────┘
                              │
                    polls HKLM Registry
                              │
                              ▼
                  ┌───────────────────────┐
                  │ Tauri Rust Backend    │
                  │ (earbuds-tracker.exe) │
                  └───────────┬───────────┘
                              │
         WASAPI / SPP / SQLITE│Tauri IPC
                              ▼
                  ┌───────────────────────┐
                  │ HTML5 / CSS / JS UI   │
                  │ (Tauri WebView2)      │
                  └───────────────────────┘
```

1. **Background Launcher (`BluetoothLauncher.ahk`)**: A lightweight AHK script that polls registry keys for active audio endpoints and launches the Tauri process in `Hide` mode as soon as the earbuds connect.
2. **Tauri Backend (Rust)**: Manages hardware polling threads, serial communication, SQLite data mapping, and local notifications.
3. **Frontend Dashboard (HTML/CSS/JS)**: A dark-themed, glassmorphic UI loaded via WebView2 for viewing statistics and history.

---

## 📂 Project Structure

```
├── BluetoothLauncher.ahk      # AutoHotkey script for background startup/polling
├── BluetoothLauncherDebug.ahk # Logging version of the AHK script
├── test_winrt.ps1             # WinRT Bluetooth prototyping script
├── test.ps1                   # PnP device battery prototyping script
└── earbuds-tracker-tauri/     # Tauri Project root
    ├── src/                   # Frontend assets (index.html, styles.css, main.js)
    └── src-tauri/             # Rust source code
        ├── src/
        │   ├── main.rs        # Tauri entrypoint
        │   ├── lib.rs         # Commands and tray setup
        │   ├── audio.rs       # WASAPI peak audio monitoring
        │   ├── bluetooth.rs   # Audio/PnP connection detection
        │   ├── db.rs          # SQLite migrations and querying
        │   ├── spp.rs         # Serial port protocol for battery stats
        │   └── tracker.rs     # Core session state driver
        └── Cargo.toml         # Rust backend dependencies
```

---

## 🚀 Installation & Setup

### Prerequisites
1. **Windows 10 / 11**
2. **AutoHotkey v2** (for background launching)
3. **Node.js** (for Tauri frontend tooling) & **Rust/Cargo toolchain** (for building the Rust backend)

### Build / Run (Developer)
1. Clone the repository:
   ```bash
   git clone https://github.com/gopi470/EarBuds-Tracker.git
   cd EarBuds-Tracker
   ```

2. Build & run the Tauri app:
   ```bash
   cd earbuds-tracker-tauri
   npm install
   npm run tauri dev
   ```

### Background Launcher (AutoHotkey)
The `BluetoothLauncher.ahk` script is designed to start the desktop app automatically when earbuds audio routing becomes active.

### Production Build
From the Tauri folder:
```bash
cd earbuds-tracker-tauri
npm run tauri build
```

---

### Developer Installation


1. **Clone the repository:**
   ```bash
   git clone https://github.com/gopi470/EarBuds-Tracker.git
   cd EarBuds-Tracker
   ```

2. **Navigate to the Tauri project:**
   ```bash
   cd earbuds-tracker-tauri
   ```

3. **Install dependencies:**
   ```bash
   npm install
   ```

4. **Run the application in development mode:**
   ```bash
   npm run tauri dev
   ```

5. **Build for production:**
   ```bash
   npm run tauri build
   ```

---

## 🛠️ Configuration Details

- **Database Path**: All usage history is persisted to a local SQLite database at:
  `%APPDATA%\EarbudsTracker\tracker.db`
- **Target Device Preferences**: The target Bluetooth device name is written to:
  `%APPDATA%\EarbudsTracker\target_device.txt`
- **On Connect/Disconnect Script Hooks**: If the backend detects a transition, it will automatically search for and execute the following custom scripts if they exist in the executable directory:
  - `on_connect` (`.ahk` / `.bat` / `.cmd` / `.ps1`)
  - `on_disconnect` (`.ahk` / `.bat` / `.cmd` / `.ps1`)

---

## ⚖️ License & Legal

This project is not affiliated with, sponsored by, or endorsed by **Nothing Technology Limited** or **CMF**. All brand names, logos, and trademarks are the property of their respective owners.
