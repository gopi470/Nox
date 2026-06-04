$devices = Get-PnpDevice | Where-Object { $_.FriendlyName -like '*CMF Buds 2a*' }
foreach ($dev in $devices) {
    $props = Get-PnpDeviceProperty -InstanceId $dev.InstanceId
    foreach ($prop in $props) {
        if ($prop.KeyName -match 'Battery' -or ($prop.Data -match '100') -or ($prop.Data -match '90')) {
            Write-Host "InstanceId: $($dev.InstanceId)"
            Write-Host "KeyName: $($prop.KeyName)"
            Write-Host "Data: $($prop.Data)"
            Write-Host "Type: $($prop.Type)"
            Write-Host "---"
        }
    }
}
