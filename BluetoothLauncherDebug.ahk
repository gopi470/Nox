#Requires AutoHotkey v2.0
#SingleInstance Force

logFile := A_ScriptDir "\launcher_debug.log"
FileAppend("Script started at " FormatTime(, "HH:mm:ss") "`n", logFile)

PollInterval := 3000
SetTimer(CheckConnection, PollInterval)

CheckConnection() {
    global logFile
    timestamp := FormatTime(, "HH:mm:ss")

    TargetDevice := "CMF Buds 2a"
    try {
        appdata := EnvGet("APPDATA")
        deviceFile := appdata "\EarbudsTracker\target_device.txt"
        if FileExist(deviceFile) {
            content := Trim(FileRead(deviceFile))
            if (content != "")
                TargetDevice := content
        }
    }

    connected := false
    try {
        for subkey in ["Render", "Capture"] {
            parentKey := "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\" subkey
            loop reg, parentKey, "K" {
                guidKey := A_LoopRegName
                propertiesKey := parentKey "\" guidKey "\Properties"
                deviceMatches := false
                loop reg, propertiesKey, "V" {
                    try {
                        val := RegRead(propertiesKey, A_LoopRegName)
                        if InStr(val, TargetDevice) {
                            deviceMatches := true
                            break
                        }
                    }
                }
                if (deviceMatches) {
                    try {
                        state := RegRead(parentKey "\" guidKey, "DeviceState")
                        if (state = 1) {
                            connected := true
                            break 2
                        }
                    }
                }
            }
        }
    }

    debugExe := A_ScriptDir "\earbuds-tracker-tauri\src-tauri\target\debug\earbuds-tracker.exe"
    exeExists   := FileExist(debugExe) != "" ? "YES" : "NO"
    procRunning := ProcessExist("earbuds-tracker.exe") ? "YES" : "NO"
    connStr     := connected ? "YES" : "NO"

    action := "NOT_CONNECTED"
    if (connected) {
        if !ProcessExist("earbuds-tracker.exe") {
            if FileExist(debugExe) {
                try {
                    Run(debugExe, A_ScriptDir "\earbuds-tracker-tauri", "Hide")
                    action := "LAUNCHED"
                } catch as e {
                    action := "LAUNCH_FAILED: " e.Message
                }
            } else {
                action := "EXE_NOT_FOUND"
            }
        } else {
            action := "ALREADY_RUNNING"
        }
    }

    line := timestamp " | Dev=" TargetDevice " | Conn=" connStr " | Exe=" exeExists " | Proc=" procRunning " | " action
    FileAppend(line "`n", logFile)
}
