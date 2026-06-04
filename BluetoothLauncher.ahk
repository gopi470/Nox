#Requires AutoHotkey v2.0
#SingleInstance Force
#NoTrayIcon

PollInterval := 3000

; Add launcher to Windows Startup registry
RegWrite('"' A_ScriptFullPath '"', "REG_SZ", "HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "EarbudsTrackerLauncher")

SetTimer(CheckConnection, PollInterval)

CheckConnection() {
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

    ; TEMP: Force debug build only
    debugExe := A_ScriptDir "\earbuds-tracker-tauri\src-tauri\target\debug\earbuds-tracker.exe"
    if (connected) {
        if !ProcessExist("earbuds-tracker.exe") {
            if FileExist(debugExe) {
                try {
                    Run(debugExe, A_ScriptDir "\earbuds-tracker-tauri", "Hide")
                }
            }
        }
    }
}
