# Verifies what the NSIS installer actually contains, by unpacking it with 7z:
# - discord-bridge.exe and the helper service are byte for byte the binaries
#   built in this job;
# - the Flowseal bundle manifest is the one of this commit and every file it
#   lists is present with the listed SHA-256 and size;
# - licenses and notices are present.
# Usage: ./scripts/ci/verify-installer.ps1 -Setup <setup.exe>
param([Parameter(Mandatory = $true)][string]$Setup)
$ErrorActionPreference = "Stop"

function Sha($p) { (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLower() }
function FindOne($root, $name) {
  $f = @(Get-ChildItem -LiteralPath $root -Recurse -File | Where-Object { $_.Name -eq $name })
  if ($f.Count -ne 1) { throw "expected exactly one $name inside the installer, found $($f.Count)" }
  $f[0]
}

$out = Join-Path $env:RUNNER_TEMP ("setup-verify-" + [guid]::NewGuid().ToString("N"))
7z x $Setup "-o$out" -y | Out-Null
if ($LASTEXITCODE -ne 0) { throw "7z could not unpack $Setup" }

# The Tauri bundler stamps the bundle type into the packaged copy of the app
# exe: the marker __TAURI_BUNDLE_TYPE_VAR_UNK (tauri-utils platform.rs)
# becomes __TAURI_BUNDLE_TYPE_VAR_NSS. That marker is the only difference
# allowed; every other byte must be the binary built in this job.
function SameExceptBundleMarker($packaged, $built) {
  $enc = [System.Text.Encoding]::Latin1
  $a = $enc.GetString([System.IO.File]::ReadAllBytes($packaged))
  $b = $enc.GetString([System.IO.File]::ReadAllBytes($built))
  if ($a.Length -ne $b.Length) { return $false }
  $unk = "__TAURI_BUNDLE_TYPE_VAR_UNK"; $nss = "__TAURI_BUNDLE_TYPE_VAR_NSS"
  $i = $b.IndexOf($unk, [System.StringComparison]::Ordinal)
  if ($i -lt 0 -or $b.IndexOf($unk, $i + 1, [System.StringComparison]::Ordinal) -ge 0) { throw "built exe: expected exactly one bundle-type marker" }
  if ($a -ceq $b) { return $true }
  if ($a.Substring($i, $nss.Length) -cne $nss) { return $false }
  return ($a.Substring(0, $i) + $unk + $a.Substring($i + $unk.Length)) -ceq $b
}

$app = FindOne $out "discord-bridge.exe"
if (-not (SameExceptBundleMarker $app.FullName "target/release/discord-bridge.exe")) { throw "discord-bridge.exe inside the installer is not the binary built in this job" }
Write-Host "ok  discord-bridge.exe = target/release/discord-bridge.exe byte for byte (except the NSIS bundle-type marker)"
$helper = FindOne $out "discord-bridge-helper.exe"
if ((Sha $helper.FullName) -ne (Sha "target/release/discord-bridge-helper.exe")) { throw "discord-bridge-helper.exe inside the installer is not the binary built in this job" }
Write-Host "ok  discord-bridge-helper.exe = target/release/discord-bridge-helper.exe ($(Sha $helper.FullName))"

$lock = Get-Content resources/flowseal.lock.json -Raw | ConvertFrom-Json
$repoManifest = "resources/flowseal/$($lock.version)/bundle-manifest.json"
$manifests = @(Get-ChildItem -LiteralPath $out -Recurse -File -Filter bundle-manifest.json |
  Where-Object { $_.Directory.Name -eq $lock.version })
if ($manifests.Count -ne 1) { throw "expected one Flowseal $($lock.version) bundle manifest, found $($manifests.Count)" }
$m = $manifests[0]
if ((Sha $m.FullName) -ne (Sha $repoManifest)) { throw "bundle-manifest.json inside the installer differs from $repoManifest" }
$manifest = Get-Content -LiteralPath $m.FullName -Raw | ConvertFrom-Json
if ($manifest.sourceSha256 -ne $lock.sha256) { throw "bundle manifest is not for the pinned Flowseal archive" }
$n = 0
foreach ($f in $manifest.files) {
  $p = Join-Path $m.Directory.FullName ($f.path -replace '/', '\')
  if (-not (Test-Path -LiteralPath $p)) { throw "Flowseal file missing in the installer: $($f.path)" }
  if ((Get-Item -LiteralPath $p).Length -ne $f.size -or (Sha $p) -ne $f.sha256) { throw "Flowseal file differs from the manifest: $($f.path)" }
  $n++
}
Write-Host "ok  Flowseal $($lock.version): $n files match bundle-manifest.json (archive $($lock.sha256))"

# Telegram WS Proxy engine: exactly the file this build produced (and whose
# hash is compiled into discord-bridge.exe), its manifest and licenses.
$tgBuilt = "resources/tg-ws-proxy/discord-bridge-tg-ws-proxy.exe"
$tgManifest = Get-Content "resources/tg-ws-proxy/tg-ws-proxy.json" -Raw | ConvertFrom-Json
$tg = FindOne $out "discord-bridge-tg-ws-proxy.exe"
if ((Sha $tg) -ne (Sha $tgBuilt) -or (Sha $tg) -ne $tgManifest.sha256) { throw "discord-bridge-tg-ws-proxy.exe in the installer differs from the build" }
$tgDir = Split-Path $tg -Parent
foreach ($f in @("tg-ws-proxy.json", "tg-ws-proxy-LICENSE.txt", "licenses/Python-LICENSE.txt", "licenses/cryptography", "licenses/certifi")) {
  if (-not (Test-Path (Join-Path $tgDir $f))) { throw "tg-ws-proxy/$f missing in the installer" }
}
Write-Host "ok  Telegram WS Proxy $($tgManifest.version) ($($tgManifest.upstreamCommit.Substring(0,7))): $($tgManifest.size) bytes, sha256 $($tgManifest.sha256), licenses present"

foreach ($name in @("LICENSE", "THIRD_PARTY_NOTICES.md", "WinDivert-LICENSE.txt")) {
  if (-not (Get-ChildItem -LiteralPath $out -Recurse -File | Where-Object { $_.Name -eq $name })) { throw "$name missing in the installer" }
  Write-Host "ok  $name present"
}
Remove-Item -LiteralPath $out -Recurse -Force
exit 0
