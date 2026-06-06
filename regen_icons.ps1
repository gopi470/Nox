Add-Type -AssemblyName System.Drawing

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$sourceCandidates = @(
  Join-Path $repoRoot 'earbuds-tracker-tauri\src-tauri\icons-new\icon-transparent.png'
  Join-Path $repoRoot 'earbuds-tracker-tauri\src-tauri\icons-new\icon.jpg'
  Join-Path $repoRoot 'earbuds-tracker-tauri\src\assets\icon.png'
  Join-Path $repoRoot 'ear-web\res\icons\1024x1024.png'
)

$source = $sourceCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $source) {
  throw "No icon source found."
}

$outDir = Join-Path $repoRoot 'earbuds-tracker-tauri\src-tauri\icons'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

function Get-SourceBitmap {
  param([string]$Path)

  $srcImg = [System.Drawing.Bitmap]::FromFile($Path)
  if ($srcImg.PixelFormat -eq [System.Drawing.Imaging.PixelFormat]::Format32bppArgb) {
    return $srcImg
  }

  $converted = New-Object System.Drawing.Bitmap $srcImg.Width, $srcImg.Height, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($converted)
  $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
  $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.Clear([System.Drawing.Color]::Transparent)
  $g.DrawImage($srcImg, 0, 0, $srcImg.Width, $srcImg.Height)
  $g.Dispose()
  $srcImg.Dispose()
  return $converted
}

function Get-Bounds {
  param([System.Drawing.Bitmap]$Bitmap)

  $left = $Bitmap.Width
  $top = $Bitmap.Height
  $right = -1
  $bottom = -1

  for ($y = 0; $y -lt $Bitmap.Height; $y++) {
    for ($x = 0; $x -lt $Bitmap.Width; $x++) {
      $c = $Bitmap.GetPixel($x, $y)
      if ($c.A -gt 0) {
        if ($x -lt $left) { $left = $x }
        if ($y -lt $top) { $top = $y }
        if ($x -gt $right) { $right = $x }
        if ($y -gt $bottom) { $bottom = $y }
      }
    }
  }

  if ($right -lt 0) {
    return $null
  }

  return [System.Drawing.Rectangle]::FromLTRB($left, $top, $right + 1, $bottom + 1)
}

function New-TrimmedCanvas {
  param(
    [System.Drawing.Bitmap]$Bitmap,
    [double]$FillRatio = 0.94
  )

  $bounds = Get-Bounds -Bitmap $Bitmap
  if (-not $bounds) {
    $bounds = New-Object System.Drawing.Rectangle 0, 0, $Bitmap.Width, $Bitmap.Height
  }

  $cropped = $Bitmap.Clone($bounds, $Bitmap.PixelFormat)
  $canvas = New-Object System.Drawing.Bitmap 1024, 1024, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($canvas)
  $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
  $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.Clear([System.Drawing.Color]::Transparent)

  $target = [int](1024 * $FillRatio)
  if ($target -lt 1) { $target = 1 }
  $scale = [Math]::Min($target / $cropped.Width, $target / $cropped.Height)
  $drawW = [int][Math]::Round($cropped.Width * $scale)
  $drawH = [int][Math]::Round($cropped.Height * $scale)
  $x = [int][Math]::Round((1024 - $drawW) / 2)
  $y = [int][Math]::Round((1024 - $drawH) / 2)

  $g.DrawImage($cropped, $x, $y, $drawW, $drawH)
  $g.Dispose()
  $cropped.Dispose()
  return $canvas
}

$baseImg = Get-SourceBitmap -Path $source
$canvas = New-TrimmedCanvas -Bitmap $baseImg -FillRatio 0.96

function Save-Png {
  param(
    [System.Drawing.Bitmap]$Canvas,
    [int]$Size,
    [string]$Path
  )

  $bmp = New-Object System.Drawing.Bitmap $Size, $Size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
  $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.Clear([System.Drawing.Color]::Transparent)
  $g.DrawImage($Canvas, 0, 0, $Size, $Size)
  $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose()
  $bmp.Dispose()
}

function Get-PngBytes {
  param(
    [System.Drawing.Bitmap]$Canvas,
    [int]$Size
  )

  $bmp = New-Object System.Drawing.Bitmap $Size, $Size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
  $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.Clear([System.Drawing.Color]::Transparent)
  $g.DrawImage($Canvas, 0, 0, $Size, $Size)

  $ms = New-Object System.IO.MemoryStream
  $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
  $bytes = $ms.ToArray()

  $ms.Dispose()
  $g.Dispose()
  $bmp.Dispose()

  return $bytes
}

Save-Png -Canvas $canvas -Size 32  -Path (Join-Path $outDir '32x32.png')
Save-Png -Canvas $canvas -Size 128 -Path (Join-Path $outDir '128x128.png')
Save-Png -Canvas $canvas -Size 256 -Path (Join-Path $outDir '128x128@2x.png')

# Build a standards-compliant ICO with PNG-compressed image entries.
$iconSizes = @(16, 32, 48, 128, 256)
$iconFrames = foreach ($size in $iconSizes) {
  [pscustomobject]@{
    Size = $size
    Bytes = (Get-PngBytes -Canvas $canvas -Size $size)
  }
}

$iconPath = Join-Path $outDir 'icon.ico'
$stream = [System.IO.File]::Open($iconPath, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write)
$writer = New-Object System.IO.BinaryWriter($stream)

$writer.Write([uint16]0) # reserved
$writer.Write([uint16]1) # icon type
$writer.Write([uint16]$iconFrames.Count)

$directoryBytes = 6 + (16 * $iconFrames.Count)
$offset = $directoryBytes

foreach ($frame in $iconFrames) {
  $size = [int]$frame.Size
  $bytes = [byte[]]$frame.Bytes
  $dim = if ($size -ge 256) { 0 } else { $size }
  $writer.Write([byte]$dim)
  $writer.Write([byte]$dim)
  $writer.Write([byte]0) # color count
  $writer.Write([byte]0) # reserved
  $writer.Write([uint16]1) # color planes
  $writer.Write([uint16]32) # bits per pixel
  $writer.Write([uint32]$bytes.Length)
  $writer.Write([uint32]$offset)
  $offset += $bytes.Length
}

foreach ($frame in $iconFrames) {
  $writer.Write([byte[]]$frame.Bytes)
}

$writer.Flush()
$writer.Dispose()
$stream.Dispose()

$canvas.Dispose()
$baseImg.Dispose()

Get-ChildItem $outDir -File | Select-Object Name,Length
