# Windows smoke test: start the real app, screenshot it over time, measure memory in normal and
# lagless-only mode, print nova.log. Screenshots are printed to the log as base64 between
# markers (no artifact storage needed). Run from the repo root after `npx tauri build`.
$ErrorActionPreference = "Continue"
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$exe = (Resolve-Path "src-tauri\target\release\nova.exe").Path
$world = "$env:RUNNER_TEMP\nova-world"
python e2e\fixtures.py $world | Out-Null
New-Item -ItemType Directory -Force "$world\Games\Starfall" | Out-Null
Copy-Item $exe "$world\Games\Starfall\Starfall.exe"
$env:NOVA_DATA_DIR = "$env:RUNNER_TEMP\nova-data"
$env:NOVA_STEAM_PATH = "$world\Steam"
$env:NOVA_EPIC_MANIFESTS = "$world\Epic\Manifests"
$env:NOVA_LAGLESS_DIR = "$world\lagless"
Remove-Item -Recurse -Force $env:NOVA_DATA_DIR -ErrorAction SilentlyContinue

function Shot($name) {
  $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
  $small = New-Object System.Drawing.Bitmap $bmp, ([int]($b.Width * 0.75)), ([int]($b.Height * 0.75))
  $ms = New-Object System.IO.MemoryStream
  $codec = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() | Where-Object { $_.MimeType -eq "image/jpeg" }
  $p = New-Object System.Drawing.Imaging.EncoderParameters 1
  $p.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter ([System.Drawing.Imaging.Encoder]::Quality), 70L
  $small.Save($ms, $codec, $p)
  Write-Output "=====SHOT $name====="
  Write-Output ([Convert]::ToBase64String($ms.ToArray()))
  Write-Output "=====END====="
}

function Memory($label) {
  $procs = @(Get-Process nova -ErrorAction SilentlyContinue) + @(Get-Process msedgewebview2 -ErrorAction SilentlyContinue)
  $ws = ($procs | Measure-Object WorkingSet64 -Sum).Sum / 1MB
  $priv = ($procs | Measure-Object PrivateMemorySize64 -Sum).Sum / 1MB
  $nova = Get-Process nova -ErrorAction SilentlyContinue | Select-Object -First 1
  Write-Output ("MEMORY {0}: {1} processes, working set {2:N0} MB, private {3:N0} MB (nova.exe alone: {4:N0} MB, CPU {5:N1}s)" -f $label, $procs.Count, $ws, $priv, ($nova.WorkingSet64 / 1MB), $nova.CPU)
}

Write-Output "WebView2: $((Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue).pv)"
Get-CimInstance Win32_VideoController | ForEach-Object { Write-Output "GPU: $($_.Name) driver $($_.DriverVersion)" }

$p = Start-Process $exe -PassThru
foreach ($t in 1, 3, 6, 12) {
  Start-Sleep -Seconds ($t - ($(if ($t -eq 1) { 0 } elseif ($t -eq 3) { 1 } elseif ($t -eq 6) { 3 } else { 6 })))
  Shot "normal-${t}s"
}
Memory "normal (library open)"
Start-Sleep -Seconds 10
Shot "normal-22s"
Memory "normal after 22s"
Stop-Process -Name nova -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 3

$p = Start-Process $exe -ArgumentList "--lagless-only" -PassThru
Start-Sleep -Seconds 10
Memory "lagless-only"
Shot "lagless-only"
$web = @(Get-Process msedgewebview2 -ErrorAction SilentlyContinue).Count
Write-Output "CHECK lagless-only has no WebView2 processes: $(if ($web -eq 0) { 'PASS' } else { "FAIL ($web)" })"
# Launching Nova again while it's in lagless-only mode should bring the library back.
Start-Process $exe | Out-Null
Start-Sleep -Seconds 8
$novas = @(Get-Process nova -ErrorAction SilentlyContinue).Count
$web = @(Get-Process msedgewebview2 -ErrorAction SilentlyContinue).Count
Write-Output "CHECK second launch hands off (one nova.exe): $(if ($novas -eq 1) { 'PASS' } else { "FAIL ($novas)" })"
Write-Output "CHECK library reopened (WebView2 running): $(if ($web -gt 0) { 'PASS' } else { 'FAIL' })"
Shot "reopened"
Memory "after reopening the library"
Stop-Process -Name nova -Force -ErrorAction SilentlyContinue

Write-Output "=====NOVA.LOG====="
Get-Content "$env:NOVA_DATA_DIR\nova.log" -ErrorAction SilentlyContinue
Write-Output "=====END LOG====="
