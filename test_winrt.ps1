Add-Type -AssemblyName System.Runtime.WindowsRuntime
$asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() | ? { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
Function Await($WinRtTask, $ResultType) {
    $asTask = $asTaskGeneric.MakeGenericMethod($ResultType)
    $netTask = $asTask.Invoke($null, @($WinRtTask))
    $netTask.Wait(-1) | Out-Null
    $netTask.Result
}

[Windows.Devices.Bluetooth.BluetoothDevice, Windows.Devices.Bluetooth, ContentType=WindowsRuntime] | Out-Null
$devices = Get-PnpDevice -Class Bluetooth | Where-Object FriendlyName -match 'CMF Buds 2a'

foreach ($dev in $devices) {
    $macString = $dev.InstanceId -replace '.*&([0-9A-F]{12})_.*', '$1'
    if ($macString -match '^[0-9A-F]{12}$') {
        $mac = [Convert]::ToUInt64($macString, 16)
        Write-Host "Querying MAC $macString ($mac)..."
        $op = [Windows.Devices.Bluetooth.BluetoothDevice]::FromBluetoothAddressAsync($mac)
        $btDevice = Await $op ([Windows.Devices.Bluetooth.BluetoothDevice])
        if ($btDevice) {
            Write-Host "Name: $($btDevice.Name)"
            Write-Host "ConnectionStatus: $($btDevice.ConnectionStatus)"
            # Trying to read battery via generic attributes or similar
        }
    }
}
