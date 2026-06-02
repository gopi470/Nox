#Requires AutoHotkey v2.0
#SingleInstance Force
#NoTrayIcon ; Runs completely silently in the background

TargetDevice := "CMF Buds 2a"
TrackerPath := A_ScriptDir "\EarbudsTracker\run.bat"
PollInterval := 3000

; Add launcher to Windows Startup registry so it runs automatically at boot
RegWrite('"' A_ScriptFullPath '"', "REG_SZ", "HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "EarbudsTrackerLauncher")

SetTimer(CheckConnection, PollInterval)

CheckConnection() {
    global TargetDevice, TrackerPath
    
    connected := false
    try {
        wmi := ComObjGet("winmgmts:")
        query := "SELECT Name, DeviceID, ConfigManagerErrorCode FROM Win32_PnPEntity WHERE DeviceID LIKE 'BTHENUM%'"
        for dev in wmi.ExecQuery(query) {
            if InStr(dev.Name, TargetDevice) && dev.ConfigManagerErrorCode = 0 {
                connected := true
                break
            }
        }
    } catch {
        connected := false
    }

    if (connected) {
        DetectHiddenWindows(true)
        ; Match exactly the Tkinter window class and title to prevent matching VS Code or Explorer windows
        if !WinExist("EarbudsTracker ahk_class TkTopLevel") {
            try {
                Run(TrackerPath, A_ScriptDir "\EarbudsTracker", "Hide")
            }
        }
    }
}
