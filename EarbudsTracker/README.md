# EarbudsTracker

A lightweight Windows desktop app that precisely tracks **connection time** and **audio playback time** for your **CMF Buds 2a** Bluetooth earbuds.

---

## Features

| Feature | Details |
|---|---|
| Bluetooth detection | WMI + Registry dual-strategy polling |
| Audio playback detection | WASAPI `IAudioMeterInformation` (system-wide) |
| Pause grace period | 5 s of silence before "playing" flips to "paused" |
| Statistics | Current session · Today · This week · Lifetime |
| Session history | SQLite database with start/end/durations |
| System tray | Color-coded icon (🔴 off / 🟡 connected / 🟢 playing) |
| Auto-start | Task Scheduler installer included |
| Crash safety | Live DB write every 30 s |

---

## Quick Start

### 1. Prerequisites

Python 3.12+ from python.org (NOT MSYS2 Python)

### 2. Install dependencies

```
cd EarbudsTracker
C:\Users\HP\AppData\Local\Programs\Python\Python313\python.exe -m pip install -r requirements.txt
```

### 3. Run

Double-click **run.bat**, or from a terminal:

```
C:\Users\HP\AppData\Local\Programs\Python\Python313\pythonw.exe main.py
```

pythonw.exe suppresses the console window.
Use python.exe if you want to see log output in a terminal.

### 4. Open the dashboard

Right-click the headphone icon in the **system tray → Open Dashboard**.

### 5. Auto-Launch on Connection (Recommended)

To make `EarbudsTracker` start automatically *only* when your CMF Buds 2a connects, use the provided AutoHotkey background launcher:
- Run **`BluetoothLauncher.ahk`** in your workspace folder.
- It runs completely silently in the background (hidden from taskbar, ~2MB RAM).
- It registers itself to Windows Startup (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) automatically upon execution.
- When it detects that the earbuds connect, it checks if `EarbudsTracker` is already running. If not, it launches it silently.

---

## File Structure

```
AutoHotkey/
├── BluetoothLauncher.ahk   Background listener script (auto-starts tracker)
├── on_connect.ahk          User hook script run when earbuds connect
├── on_disconnect.ahk       User hook script run when earbuds disconnect
└── EarbudsTracker/         Main application files
    ├── main.py              Entry point and lifecycle management
    ├── tracker.py           Session state machine
    ├── bluetooth_monitor.py BT connection detection
    ├── audio_monitor.py     WASAPI audio-level detection
    ├── database.py          SQLite persistence layer
    ├── ui.py                Tkinter dashboard UI
    ├── tray.py              System-tray icon
    ├── requirements.txt
    ├── run.bat              One-click launcher
    └── install_startup.bat  Task Scheduler installer (alternative to AHK launcher)
```

Data is stored in:
- `%APPDATA%\EarbudsTracker\tracker.db`
- `%APPDATA%\EarbudsTracker\tracker.log`

---

## Connection/Disconnection Auto-Run Scripts

Whenever the target earbuds connect or disconnect, the application will look for and automatically run your custom scripts.

- **Trigger Scripts:** The tracker searches for `on_connect.ahk` or `on_disconnect.ahk` (also supporting `.bat`, `.cmd`, `.ps1`, and `.py`) in:
  1. The parent workspace folder: `c:\Users\HP\Documents\AutoHotkey\`
  2. The application directory: `c:\Users\HP\Documents\AutoHotkey\EarbudsTracker\`
- **Default Scripts:** Pre-created templates `on_connect.ahk` and `on_disconnect.ahk` are in your workspace. You can edit them to launch Spotify, change audio outputs, adjust system volume, send inputs, or perform any other task.

---

## How Connection Detection Works

**Primary — WASAPI Active Audio Endpoint (Most Reliable):**
Windows dynamically registers active render endpoints when a Bluetooth audio device links up. The tracker checks if any `DEVICE_STATE_ACTIVE` endpoint's name contains `"CMF Buds 2a"`. This approach is handled by the kernel and has zero false-positives.

**Secondary — WMI BTHENUM Filter:**
If the COM service falls back, the tracker queries WMI `Win32_PnPEntity` but specifically filters for `DeviceID LIKE 'BTHENUM%'`. Unlike generic `BTH` registry entries which stay active when paired but off, `BTHENUM` entries are only present when there is a live connection.

**Fallback — Broad WMI Check:**
As a last resort, a broad WMI search by name is executed if other interfaces fail.

**Why polling instead of events?**
WMI event subscriptions for Bluetooth devices are unreliable across Windows versions and sleep/wake cycles. A 3-second poll costs ~0.1 ms and is rock-solid.

---

## How Playback Detection Works

**Windows Core Audio API — IAudioMeterInformation:**

1. The app enumerates all active audio render endpoints via `AudioUtilities.GetAllDevices()`.
2. It finds the endpoint whose `FriendlyName` contains `"CMF Buds 2a"`.
3. On that endpoint it activates the `IAudioMeterInformation` COM interface.
4. Every **1 second** it reads `GetPeakValue()` — the instantaneous peak amplitude (0.0 = silence, 1.0 = full scale).
5. If `peak > 0.001` → audio is **playing**.
6. If `peak <= 0.001` for **5 consecutive checks** (5 seconds) → state flips to **paused**.

**Why this method is accurate:**
- `IAudioMeterInformation` measures the actual digital samples delivered to the hardware — not application mute state, not OS volume.
- A paused media player, a muted browser tab, or complete silence on a stream all yield peak = 0.
- Works for **any** audio source: Spotify, YouTube, VLC, Discord, games, system sounds.
- No app-specific APIs required.

**Hysteresis (grace period):**
The 5-second grace prevents rapid on/off toggling when audio briefly dips (gap between tracks, video buffering).

---

## Database Schema

```sql
-- One row per Bluetooth connection session
CREATE TABLE sessions (
    id              INTEGER PRIMARY KEY,
    session_start   TEXT,       -- ISO-8601
    session_end     TEXT,
    connected_secs  REAL,       -- total connection duration
    playback_secs   REAL        -- total audio playback duration
);

-- Aggregated daily totals (fast reads for stats UI)
CREATE TABLE daily_stats (
    day             TEXT PRIMARY KEY,   -- YYYY-MM-DD
    connected_secs  REAL,
    playback_secs   REAL
);
```

Export to CSV:
```
sqlite3 %APPDATA%\EarbudsTracker\tracker.db ".mode csv" ".headers on" "SELECT * FROM sessions;" > sessions.csv
```

---

## Troubleshooting

| Problem | Fix |
|---|---|
| App doesn't detect earbuds | Ensure BT device name in Windows is exactly "CMF Buds 2a". Check in Device Manager. |
| Audio playback not detected | The earbuds must be set as the **default audio output** in Windows Sound Settings. |
| ModuleNotFoundError | Run pip install with Python313\python.exe, not the MSYS2 python. |
| Tray icon missing | Check Task Manager; if pythonw.exe is running, icon may be hidden in overflow tray. |
| Want console log output | Run python.exe main.py instead of pythonw.exe. |

---

## CPU / RAM Profile

| Component | Cost |
|---|---|
| BT poll (WMI) | ~0.1 ms every 3 s |
| Audio poll (WASAPI) | ~0.05 ms every 1 s |
| UI refresh | ~1 ms every 1 s (only if window is visible) |
| DB live-write | ~2 ms every 30 s |
| **Total idle RAM** | **~25–35 MB** |
