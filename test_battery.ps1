# test_battery.ps1 - Standalone battery test for CMF Buds 2a over SPP

Write-Host "=== CMF Buds 2a Battery Test ===" -ForegroundColor Cyan

# Step 1: Find the Bluetooth device
Write-Host "`n[1] Scanning for CMF device in Bluetooth class..." -ForegroundColor Yellow
$btDevices = Get-PnpDevice -Class Bluetooth | Where-Object { $_.FriendlyName -like '*CMF*' }
if (-not $btDevices) {
    Write-Host "  ERROR: No CMF device found. Are the buds connected?" -ForegroundColor Red
    # Show all BT devices for diagnosis
    Write-Host "  All Bluetooth devices:"
    Get-PnpDevice -Class Bluetooth | Select-Object FriendlyName, InstanceId | Format-Table
    exit 1
}
$btDevices | ForEach-Object { Write-Host "  Found: $($_.FriendlyName) [$($_.InstanceId)]" -ForegroundColor Green }
$dev = $btDevices | Select-Object -First 1

# Step 2: Extract MAC and find COM port
$com = $null
if ($dev.InstanceId -match 'DEV_([0-9A-Fa-f]{12})') {
    $mac = $Matches[1]
    Write-Host "`n[2] MAC segment: $mac" -ForegroundColor Yellow
    $portDev = Get-PnpDevice -Class Ports -Status OK | Where-Object { $_.InstanceId -like "*$mac*" } | Select-Object -First 1
    if ($portDev -and $portDev.FriendlyName -match '(COM\d+)') {
        $com = $Matches[1]
        Write-Host "  SPP COM port: $com ($($portDev.FriendlyName))" -ForegroundColor Green
    } else {
        Write-Host "  No matching Ports device found for MAC $mac" -ForegroundColor Red
        Write-Host "  All available COM ports:"
        Get-PnpDevice -Class Ports -Status OK | Select-Object FriendlyName, InstanceId | Format-Table
    }
} else {
    Write-Host "  Could not parse MAC from: $($dev.InstanceId)" -ForegroundColor Red
}

if (-not $com) {
    Write-Host "`nFalling back to manual COM port list:" -ForegroundColor Yellow
    $portList = [System.IO.Ports.SerialPort]::GetPortNames()
    Write-Host "  Available ports: $($portList -join ', ')"
    $com = "COM3"
    Write-Host "  Using default: $com" -ForegroundColor Yellow
}

# Step 3: Send battery request packet and read response
Write-Host "`n[3] Opening $com at 9600 baud..." -ForegroundColor Yellow

# Battery request packet: 0x55 0x60 0x01 0x07 0xC0 0x00 0x00 0x01 0xAC 0xDF
# (CRC-16/IBM of the 8-byte header is 0xDFAC → LE bytes 0xAC, 0xDF)
$packet = [byte[]](0x55, 0x60, 0x01, 0x07, 0xC0, 0x00, 0x00, 0x01, 0xAC, 0xDF)

$port = New-Object System.IO.Ports.SerialPort($com, 9600, [System.IO.Ports.Parity]::None, 8, [System.IO.Ports.StopBits]::One)
$port.ReadTimeout  = 3000
$port.WriteTimeout = 2000

try {
    $port.Open()
    Write-Host "  Port opened OK. Settling for 1s..." -ForegroundColor Green
    Start-Sleep -Seconds 1

    $hexPkt = ($packet | ForEach-Object { '{0:X2}' -f $_ }) -join ' '
    Write-Host "  Sending: $hexPkt" -ForegroundColor Cyan

    $port.Write($packet, 0, $packet.Length)
    $port.BaseStream.Flush()

    Write-Host "  Waiting up to 3s for response..." -ForegroundColor Yellow
    $deadline = (Get-Date).AddSeconds(3)
    $allBytes = [System.Collections.Generic.List[byte]]::new()

    while ((Get-Date) -lt $deadline) {
        $avail = $port.BytesToRead
        if ($avail -gt 0) {
            $buf = New-Object byte[] $avail
            $port.Read($buf, 0, $avail) | Out-Null
            $allBytes.AddRange($buf)
        } else {
            Start-Sleep -Milliseconds 100
        }
    }

    if ($allBytes.Count -eq 0) {
        Write-Host "  No response received." -ForegroundColor Red
        Write-Host "  This may mean the device needs a different port, or the SPP link is not established."
    } else {
        $hex = ($allBytes | ForEach-Object { '{0:X2}' -f $_ }) -join ' '
        Write-Host "`n  RAW RESPONSE ($($allBytes.Count) bytes):" -ForegroundColor Cyan
        Write-Host "  $hex"

        # Parse battery if response starts with 0x55
        $arr = $allBytes.ToArray()
        for ($i = 0; $i -lt $arr.Count - 1; $i++) {
            if ($arr[$i] -eq 0x55 -and ($arr.Count - $i) -ge 10) {
                $cmd = [uint16]($arr[$i + 3] -bor ($arr[$i + 4] -shl 8))
                Write-Host ("`n  Frame at offset {0}: cmd=0x{1} ({2})" -f $i, $cmd.ToString('X4'), $cmd) -ForegroundColor Yellow
                # Battery response commands: 57345 (0xE001) or 16391 (0x4007)
                if ($cmd -eq 57345 -or $cmd -eq 16391) {
                    Write-Host "  --> Battery response detected!" -ForegroundColor Green
                    $numDevices = $arr[$i + 8]
                    Write-Host "  Num devices: $numDevices"
                    for ($d = 0; $d -lt $numDevices; $d++) {
                        $devId  = $arr[$i + 9  + $d * 2]
                        $batByte= $arr[$i + 10 + $d * 2]
                        $level  = $batByte -band 0x7F
                        $charging = ($batByte -band 0x80) -ne 0
                        $name = switch ($devId) { 2 { 'LEFT' } 3 { 'RIGHT' } 4 { 'CASE' } default { "ID=$devId" } }
                        $chStr = if ($charging) { ' (charging)' } else { '' }
                        Write-Host ("    {0}: {1}%{2}" -f $name, $level, $chStr) -ForegroundColor Green
                    }
                } else {
                    Write-Host "  (not a battery response frame)"
                }
            }
        }
    }
} catch {
    Write-Host "  EXCEPTION: $_" -ForegroundColor Red
} finally {
    if ($port.IsOpen) { $port.Close(); Write-Host "`nPort closed." }
}
