$safe_name = "TWS"
$devs = Get-PnpDevice | Where-Object { $_.FriendlyName -like "*$safe_name*" }
$found = $false
foreach ($d in $devs) {
    $val = Get-PnpDeviceProperty -InstanceId $d.InstanceId -KeyName "{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2" -ErrorAction SilentlyContinue
    if ($val -and $val.Data -ne $null) {
        Write-Host "SUCCESS: Found battery: $($val.Data)% on $($d.FriendlyName) [$($d.InstanceId)]"
        $found = $true
        break
    }
}
if (-not $found) {
    Write-Host "FAILED: Battery property not found on any matching device."
}
