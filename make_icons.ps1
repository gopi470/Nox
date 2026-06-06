Add-Type -AssemblyName System.Drawing

$src = 'C:\Users\HP\.gemini\antigravity\brain\0a02e353-7e77-4a82-bc04-a91a50bf7486\earbuds_app_icon_1780712493562.png'
$dir = 'c:\Users\HP\Documents\repos\EarBuds-Tracker\earbuds-tracker-tauri\src-tauri\icons'

$orig = [System.Drawing.Image]::FromFile($src)

$sizes = @{
    '32x32.png'             = 32
    '128x128.png'           = 128
    '128x128@2x.png'        = 256
    'icon.png'              = 512
    'Square30x30Logo.png'   = 30
    'Square44x44Logo.png'   = 44
    'Square71x71Logo.png'   = 71
    'Square89x89Logo.png'   = 89
    'Square107x107Logo.png' = 107
    'Square142x142Logo.png' = 142
    'Square150x150Logo.png' = 150
    'Square284x284Logo.png' = 284
    'Square310x310Logo.png' = 310
    'StoreLogo.png'         = 50
}

foreach ($entry in $sizes.GetEnumerator()) {
    $s = $entry.Value
    $bmp = New-Object System.Drawing.Bitmap($s, $s)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.DrawImage($orig, 0, 0, $s, $s)
    $g.Dispose()
    $outPath = Join-Path $dir $entry.Key
    $bmp.Save($outPath, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Host "Saved $($entry.Key)"
}

# Build ICO with multiple sizes embedded as PNG blobs
$icoSizes = @(16, 24, 32, 48, 64, 128, 256)
$ms = New-Object System.IO.MemoryStream
$writer = New-Object System.IO.BinaryWriter($ms)

# ICO header
$writer.Write([uint16]0)
$writer.Write([uint16]1)
$writer.Write([uint16]$icoSizes.Count)

# Collect each size as PNG bytes first
$pngBlobs = @()
foreach ($s in $icoSizes) {
    $bmp = New-Object System.Drawing.Bitmap($s, $s)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.DrawImage($orig, 0, 0, $s, $s)
    $g.Dispose()
    $pngMs = New-Object System.IO.MemoryStream
    $bmp.Save($pngMs, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    $pngBlobs += , $pngMs.ToArray()
    $pngMs.Dispose()
}

# Directory entries: 6 byte header + 16 bytes per entry
$dataOffset = 6 + $icoSizes.Count * 16
foreach ($i in 0..($icoSizes.Count - 1)) {
    $s = $icoSizes[$i]
    $sz = if ($s -ge 256) { 0 } else { $s }
    $writer.Write([byte]$sz)
    $writer.Write([byte]$sz)
    $writer.Write([byte]0)
    $writer.Write([byte]0)
    $writer.Write([uint16]1)
    $writer.Write([uint16]32)
    $writer.Write([uint32]$pngBlobs[$i].Length)
    $writer.Write([uint32]$dataOffset)
    $dataOffset += $pngBlobs[$i].Length
}
foreach ($blob in $pngBlobs) { $writer.Write($blob) }

$icoPath = Join-Path $dir 'icon.ico'
[System.IO.File]::WriteAllBytes($icoPath, $ms.ToArray())
$orig.Dispose()

Write-Host "Saved icon.ico"
Write-Host "All icons generated successfully!"
