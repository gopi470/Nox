Add-Type -AssemblyName System.Drawing

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$sourceCandidates = @(
  Join-Path $repoRoot 'earbuds-tracker-tauri\src\assets\icon.png'
  Join-Path $repoRoot 'ear-web\res\icons\1024x1024.png'
  Join-Path $repoRoot 'earbuds-tracker-tauri\src-tauri\icons-new\image.png'
)

$source = $sourceCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $source) {
  throw "No icon source found."
}

$outDir = Join-Path $repoRoot 'earbuds-tracker-tauri\src-tauri\icons'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$img = [System.Drawing.Image]::FromFile($source)

function Save-Png {
  param(
    [int]$Size,
    [string]$Path
  )

  $bmp = New-Object System.Drawing.Bitmap $Size, $Size
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
  $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.Clear([System.Drawing.Color]::Transparent)
  $g.DrawImage($img, 0, 0, $Size, $Size)
  $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose()
  $bmp.Dispose()
}

Save-Png -Size 32 -Path (Join-Path $outDir '32x32.png')
Save-Png -Size 128 -Path (Join-Path $outDir '128x128.png')
Save-Png -Size 256 -Path (Join-Path $outDir '128x128@2x.png')

$bmp = New-Object System.Drawing.Bitmap 256, 256
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
$g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
$g.Clear([System.Drawing.Color]::Transparent)
$g.DrawImage($img, 0, 0, 256, 256)

$hicon = $bmp.GetHicon()
$icon = [System.Drawing.Icon]::FromHandle($hicon)
$iconPath = Join-Path $outDir 'icon.ico'
$fs = [System.IO.File]::Create($iconPath)
$icon.Save($fs)
$fs.Close()

$g.Dispose()
$bmp.Dispose()
$img.Dispose()

Get-ChildItem $outDir -File | Select-Object Name,Length
