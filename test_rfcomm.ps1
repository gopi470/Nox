# test_rfcomm.ps1 - Connect to CMF Buds 2a via the custom SPP UUID using WinRT RFCOMM

Add-Type -AssemblyName System.Runtime.WindowsRuntime

# Helper to await WinRT async operations
$asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() |
    Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and
                   $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]

function Await($WinRtTask, $ResultType) {
    $asTask = $asTaskGeneric.MakeGenericMethod($ResultType)
    $netTask = $asTask.Invoke($null, @($WinRtTask))
    $netTask.Wait(-1) | Out-Null
    $netTask.Result
}

function AwaitAction($WinRtTask) {
    $asTask = [System.WindowsRuntimeSystemExtensions].GetMethod('AsTask', [type[]]@([Windows.Foundation.IAsyncAction]))
    $netTask = $asTask.Invoke($null, @($WinRtTask))
    $netTask.Wait(-1) | Out-Null
}

# Load WinRT types
[Windows.Devices.Bluetooth.BluetoothDevice,                   Windows.Devices.Bluetooth, ContentType=WindowsRuntime] | Out-Null
[Windows.Devices.Bluetooth.Rfcomm.RfcommDeviceService,        Windows.Devices.Bluetooth, ContentType=WindowsRuntime] | Out-Null
[Windows.Devices.Bluetooth.Rfcomm.RfcommServiceId,            Windows.Devices.Bluetooth, ContentType=WindowsRuntime] | Out-Null
[Windows.Networking.Sockets.StreamSocket,                     Windows.Networking, ContentType=WindowsRuntime] | Out-Null
[Windows.Storage.Streams.DataWriter,                          Windows.Storage, ContentType=WindowsRuntime] | Out-Null
[Windows.Storage.Streams.DataReader,                          Windows.Storage, ContentType=WindowsRuntime] | Out-Null

# CMF / Nothing custom SPP UUID (from ear-web bluetooth_socket.js)
$SPP_UUID = "aeac4a03-dff5-498f-843a-34487cf133eb"

Write-Host "=== CMF Buds 2a RFCOMM Battery Test ===" -ForegroundColor Cyan

# Find MAC from PnP
$dev = Get-PnpDevice -Class Bluetooth | Where-Object { $_.FriendlyName -like '*CMF Buds 2a' } | Select-Object -First 1
if (-not $dev) { Write-Host "Device not found!" -ForegroundColor Red; exit 1 }

if ($dev.InstanceId -notmatch 'DEV_([0-9A-Fa-f]{12})') {
    Write-Host "Cannot parse MAC from: $($dev.InstanceId)" -ForegroundColor Red; exit 1
}
$macHex = $Matches[1]
$macUInt64 = [Convert]::ToUInt64($macHex, 16)
Write-Host "Device MAC: $macHex  ($macUInt64)" -ForegroundColor Green

# Get BluetoothDevice
Write-Host "Getting BluetoothDevice..." -ForegroundColor Yellow
$btDevice = Await ([Windows.Devices.Bluetooth.BluetoothDevice]::FromBluetoothAddressAsync($macUInt64)) ([Windows.Devices.Bluetooth.BluetoothDevice])
if (-not $btDevice) { Write-Host "BluetoothDevice not found!" -ForegroundColor Red; exit 1 }
Write-Host "Device: $($btDevice.Name)  Status: $($btDevice.ConnectionStatus)" -ForegroundColor Green

# Get RFCOMM services for the custom UUID
Write-Host "Getting RFCOMM services for UUID $SPP_UUID ..." -ForegroundColor Yellow
$serviceId  = [Windows.Devices.Bluetooth.Rfcomm.RfcommServiceId]::FromUuid([Guid]$SPP_UUID)
$serviceOp  = $btDevice.GetRfcommServicesForIdAsync($serviceId,
    [Windows.Devices.Bluetooth.BluetoothCacheMode]::Uncached)
$services   = Await $serviceOp ([Windows.Devices.Bluetooth.Rfcomm.RfcommDeviceServicesResult])

Write-Host "Services error: $($services.Error)" -ForegroundColor Yellow
Write-Host "Services count: $($services.Services.Count)" -ForegroundColor Yellow

if ($services.Services.Count -eq 0) {
    Write-Host "No RFCOMM service found for UUID $SPP_UUID" -ForegroundColor Red

    # Try listing ALL rfcomm services
    Write-Host "`nListing ALL available RFCOMM services on device..." -ForegroundColor Yellow
    $allSvcOp  = $btDevice.GetRfcommServicesAsync([Windows.Devices.Bluetooth.BluetoothCacheMode]::Uncached)
    $allSvcs   = Await $allSvcOp ([Windows.Devices.Bluetooth.Rfcomm.RfcommDeviceServicesResult])
    Write-Host "All services count: $($allSvcs.Services.Count)"
    foreach ($s in $allSvcs.Services) {
        Write-Host "  UUID: $($s.ServiceId.Uuid)" -ForegroundColor Cyan
    }
    exit 1
}

$rfcommSvc = $services.Services[0]
Write-Host "Found RFCOMM service: $($rfcommSvc.ServiceId.Uuid)" -ForegroundColor Green

# Connect socket
Write-Host "Connecting StreamSocket..." -ForegroundColor Yellow
$socket = New-Object Windows.Networking.Sockets.StreamSocket
try {
    AwaitAction ($socket.ConnectAsync($rfcommSvc.ConnectionHostName, $rfcommSvc.ConnectionServiceName,
        [Windows.Networking.Sockets.SocketProtectionLevel]::BluetoothEncryptionAllowNullAuthentication))
} catch {
    Write-Host "Socket connect failed: $_" -ForegroundColor Red
    exit 1
}
Write-Host "Socket connected!" -ForegroundColor Green

# Build battery request packet
# [0x55, 0x60, 0x01, CMD_LO=0x07, CMD_HI=0xC0, PAYLOAD_LEN=0x00, 0x00, OP_ID=0x01, CRC_LO=0xAC, CRC_HI=0xDF]
$packet = [byte[]](0x55, 0x60, 0x01, 0x07, 0xC0, 0x00, 0x00, 0x01, 0xAC, 0xDF)

$writer = New-Object Windows.Storage.Streams.DataWriter($socket.OutputStream)
$writer.WriteBytes($packet)
Write-Host "Sending: $(($packet | ForEach-Object { '{0:X2}' -f $_ }) -join ' ')" -ForegroundColor Cyan
$storeOp = $writer.StoreAsync()
Await $storeOp ([uint32]) | Out-Null
$writer.DetachStream() | Out-Null

# Read response
Write-Host "Waiting for response (5s)..." -ForegroundColor Yellow
$reader = New-Object Windows.Storage.Streams.DataReader($socket.InputStream)
$reader.InputStreamOptions = [Windows.Storage.Streams.InputStreamOptions]::Partial

$deadline = (Get-Date).AddSeconds(5)
$allBytes  = [System.Collections.Generic.List[byte]]::new()

while ((Get-Date) -lt $deadline -and $allBytes.Count -lt 32) {
    if ($reader.UnconsumedBufferLength -eq 0) {
        $loadOp = $reader.LoadAsync(256)
        $task = $asTaskGeneric.MakeGenericMethod([uint32]).Invoke($null, @($loadOp))
        if ($task.Wait(500)) {
            $avail = $task.Result
        } else {
            $avail = 0
        }
    } else {
        $avail = $reader.UnconsumedBufferLength
    }
    
    if ($avail -gt 0) {
        for ($i = 0; $i -lt $avail; $i++) { $allBytes.Add($reader.ReadByte()) }
    }
}

$socket.Dispose()

if ($allBytes.Count -eq 0) {
    Write-Host "No response received." -ForegroundColor Red
} else {
    $hex = ($allBytes | ForEach-Object { '{0:X2}' -f $_ }) -join ' '
    Write-Host "`nRAW RESPONSE ($($allBytes.Count) bytes): $hex" -ForegroundColor Cyan

    $arr = $allBytes.ToArray()
    for ($i = 0; $i -lt $arr.Count; $i++) {
        if ($arr[$i] -eq 0x55 -and ($arr.Count - $i) -ge 10) {
            $cmd = [uint16]($arr[$i+3] -bor ($arr[$i+4] -shl 8))
            Write-Host ("Frame cmd=0x{0} ({1})" -f $cmd.ToString('X4'), $cmd) -ForegroundColor Yellow
            if ($cmd -eq 57345 -or $cmd -eq 16391) {
                Write-Host "Battery response!" -ForegroundColor Green
                $n = $arr[$i+8]
                for ($d = 0; $d -lt $n; $d++) {
                    $did = $arr[$i+9+$d*2]
                    $bb  = $arr[$i+10+$d*2]
                    $lvl = $bb -band 0x7F
                    $chg = ($bb -band 0x80) -ne 0
                    $nm  = switch ($did) { 2{'LEFT'} 3{'RIGHT'} 4{'CASE'} default{"ID=$did"} }
                    Write-Host ("  {0}: {1}%{2}" -f $nm, $lvl, $(if($chg){' (charging)'}else{''})) -ForegroundColor Green
                }
            }
        }
    }
}
