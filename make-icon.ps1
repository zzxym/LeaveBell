# Build app icon: source logo + small black "e" badge at bottom-right.
# Outputs: assets/app-icon.png (512x512, runtime window icon) and assets/app.ico (Windows binary icon).
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

$root = Split-Path $PSScriptRoot -Parent
$src = Join-Path $root "icon.png"
$assetDir = Join-Path $PSScriptRoot "assets"
New-Item -ItemType Directory -Path $assetDir -Force | Out-Null
$pngOut = Join-Path $assetDir "app-icon.png"
$icoOut = Join-Path $assetDir "app.ico"

$bmp = New-Object System.Drawing.Bitmap 512,512,([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality

$logoSrc = [System.Drawing.Image]::FromFile($src)
$g.DrawImage($logoSrc, 0, 0, 512, 512)
$logoSrc.Dispose()

# Black handwritten lowercase "e" hugging the bottom-right corner.
# Handwriting font (Segoe Script); no outline/badge so it blends into the logo.
$font = New-Object System.Drawing.Font("Segoe Script", 450, [System.Drawing.FontStyle]::Regular)
$sf = New-Object System.Drawing.StringFormat
$sf.Alignment = [System.Drawing.StringAlignment]::Center
$sf.LineAlignment = [System.Drawing.StringAlignment]::Center
$textRect = [System.Drawing.RectangleF]::new(0, 0, 512, 512)
$path = New-Object System.Drawing.Drawing2D.GraphicsPath
$path.AddString("e", $font.FontFamily, [int]$font.Style, $font.Size, $textRect, $sf)

# Translate the glyph so its ink bounds hug the bottom-right corner (2px margin).
$bounds = $path.GetBounds()
$dx = [single](512 - 2 - $bounds.Right)
$dy = [single](512 - 2 - $bounds.Bottom)
$matrix = New-Object System.Drawing.Drawing2D.Matrix
$matrix.Translate($dx, $dy)
$path.Transform($matrix)
$g.FillPath([System.Drawing.Brushes]::Black, $path)
$matrix.Dispose()
$path.Dispose()

$g.Dispose()
$bmp.Save($pngOut, [System.Drawing.Imaging.ImageFormat]::Png)

# Build multi-size ICO (PNG-compressed entries: 16,24,32,48,64,128,256).
$sizes = @(16,24,32,48,64,128,256)
$ms = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter($ms)
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
$pngParts = @()
foreach ($s in $sizes) {
  $resized = New-Object System.Drawing.Bitmap $s,$s,([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $gg = [System.Drawing.Graphics]::FromImage($resized)
  $gg.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
  $gg.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $gg.DrawImage($bmp, 0, 0, $s, $s)
  $gg.Dispose()
  $pms = New-Object System.IO.MemoryStream
  $resized.Save($pms, [System.Drawing.Imaging.ImageFormat]::Png)
  $resized.Dispose()
  $bytes = $pms.ToArray()
  $pms.Dispose()
  $pngParts += ,$bytes
  $dim = if ($s -ge 256) { 0 } else { $s }
  $bw.Write([byte]$dim); $bw.Write([byte]$dim); $bw.Write([byte]0); $bw.Write([byte]0)
  $bw.Write([uint16]1); $bw.Write([uint16]32)
  $bw.Write([uint32]$bytes.Length); $bw.Write([uint32]$offset)
  $offset += $bytes.Length
}
foreach ($bytes in $pngParts) { $bw.Write($bytes) }
$bw.Flush()
[System.IO.File]::WriteAllBytes($icoOut, $ms.ToArray())
$bw.Dispose(); $ms.Dispose(); $bmp.Dispose()

Write-Host ("PNG: {0}" -f $pngOut)
Write-Host ("ICO: {0}" -f $icoOut)
