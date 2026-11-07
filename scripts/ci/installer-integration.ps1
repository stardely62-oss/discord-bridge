# Installer integration scenarios on a real Windows (CI runner, admin).
#   ./scripts/ci/installer-integration.ps1 -Setup <new setup.exe> [-Previous <older setup.exe>]
# Every scenario checks that the installed Flowseal files are exactly one
# version (hashes equal the source bundle) — never a mix.
param([Parameter(Mandatory)][string]$Setup, [string]$Previous, [string]$Bundle = "resources/flowseal/1.10.4")
$ErrorActionPreference = 'Stop'
$inst = Join-Path $env:ProgramFiles 'Discord Bridge'
$ver = Split-Path $Bundle -Leaf
$fails = New-Object System.Collections.Generic.List[string]
function Check($ok, $msg) { if ($ok) { Write-Host "  ok   $msg" } else { Write-Host "  FAIL $msg"; $fails.Add($msg) } }
function Run($exe, [string[]]$a) { (Start-Process $exe -ArgumentList $a -Wait -PassThru).ExitCode }
function ServiceState { $s = Get-Service DiscordBridgeHelper -ErrorAction SilentlyContinue; if ($s) { "$($s.Status)" } else { 'absent' } }
$manifest = Get-Content "$Bundle/bundle-manifest.json" -Raw | ConvertFrom-Json
function BundleIntact {
  foreach ($f in $manifest.files) {
    $p = Join-Path "$inst\flowseal\$ver" ($f.path -replace '/', '\')
    if (-not (Test-Path $p)) { return "missing $($f.path)" }
    if ((Get-FileHash $p -Algorithm SHA256).Hash.ToLower() -ne $f.sha256) { return "hash $($f.path)" }
  }
  'ok'
}
function LegacyDriverLoad {
  # What versions up to alpha.7 did: winws (and so the WinDivert driver) from Program Files.
  $p = Start-Process "$inst\flowseal\$ver\bin\winws.exe" -ArgumentList '--wf-tcp=9' -WorkingDirectory "$inst\flowseal\$ver\bin" -PassThru -WindowStyle Hidden
  Start-Sleep 4
  Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
  Start-Sleep 1
  $drv = ((sc.exe query WinDivert) | Select-String 'STATE') -join ''
  Write-Host "  winws from Program Files started and stopped; driver: $drv"
}

if ($Previous) {
  Write-Host "== 1. previous release installed, legacy driver load, upgrade to the new build"
  Check ((Run $Previous @('/S')) -eq 0) "previous release installs"
  Check ((ServiceState) -eq 'Running') "previous helper running"
  LegacyDriverLoad
  $code = Run $Setup @('/S')
  Check ($code -eq 0) "upgrade succeeds (exit $code)"
  Check ((BundleIntact) -eq 'ok') "Flowseal files are exactly the new bundle ($(BundleIntact))"
  Check ((ServiceState) -eq 'Running') "new helper running"
}
else {
  Write-Host "== 1. clean install"
  $code = Run $Setup @('/S')
  Check ($code -eq 0) "clean install (exit $code)"
  Check ((BundleIntact) -eq 'ok') "Flowseal files intact"
  Check ((ServiceState) -eq 'Running') "helper running"
}

Write-Host "== 2. reinstall the same version after a legacy driver load"
LegacyDriverLoad
$code = Run $Setup @('/S')
Check ($code -eq 0) "reinstall succeeds (exit $code)"
Check ((BundleIntact) -eq 'ok') "Flowseal files intact ($(BundleIntact))"
Check ((ServiceState) -eq 'Running') "helper running"

Write-Host "== 2a. Telegram WS Proxy from Program Files: integrity, start, independence from Zapret, stop"
$tgExe = "$inst\tg-ws-proxy\discord-bridge-tg-ws-proxy.exe"
$tgMan = Get-Content "$inst\tg-ws-proxy\tg-ws-proxy.json" -Raw | ConvertFrom-Json
Check ((Test-Path $tgExe) -and ((Get-FileHash $tgExe -Algorithm SHA256).Hash.ToLower() -eq $tgMan.sha256)) "installed engine matches its manifest"
function StartTg([int]$port) {
  $psi = New-Object System.Diagnostics.ProcessStartInfo $tgExe, "--stdin-config"
  $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true
  $psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true; $psi.RedirectStandardError = $true
  $p = [System.Diagnostics.Process]::Start($psi)
  $secret = -join ((1..32) | ForEach-Object { '{0:x}' -f (Get-Random -Maximum 16) })
  $p.StandardInput.WriteLine((@{ host = "127.0.0.1"; port = $port; secret = $secret; fallbackCfproxy = $false; poolSize = 0 } | ConvertTo-Json -Compress))
  $p.StandardInput.Flush()
  $deadline = (Get-Date).AddSeconds(90); $ready = $false
  while ((Get-Date) -lt $deadline -and -not $p.HasExited) {
    $line = $p.StandardOutput.ReadLine()
    if ($null -eq $line) { break }
    if ($line -match '"event":"ready"') { $ready = $true; break }
  }
  return @{ p = $p; ready = $ready; secret = $secret }
}
$tgPort = Get-Random -Minimum 20000 -Maximum 40000
$svcBefore = ServiceState
$t = StartTg $tgPort
Check $t.ready "engine from Program Files starts listening on 127.0.0.1:$tgPort"
Check ((ServiceState) -eq $svcBefore) "helper service untouched by the proxy start ($svcBefore)"
# No window, no tray: the engine has no GUI (checked by --self-test at build) and no visible window.
Start-Sleep 1
$procs = @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq $tgExe })
Check ($procs.Count -ge 1) "engine processes run from the installed path ($($procs.Count))"
Check (@(Get-Process -Id $procs.ProcessId | Where-Object { $_.MainWindowHandle -ne 0 }).Count -eq 0) "no window of the engine"
# Zapret off/on does not stop the proxy.
$null = & "$inst\discord-bridge-helper.exe" --stop-engine 2>&1
$c = New-Object System.Net.Sockets.TcpClient
try { $c.Connect("127.0.0.1", $tgPort); $ok = $c.Connected } catch { $ok = $false } finally { $c.Close() }
Check ($ok -and -not $t.p.HasExited) "proxy keeps listening after the Zapret engine is stopped"
Start-Service DiscordBridgeHelper -ErrorAction SilentlyContinue
# Stopping the proxy (stdin closed) ends every engine process and leaves the helper as it was.
$svcNow = ServiceState
$t.p.StandardInput.Close()
Check ($t.p.WaitForExit(20000)) "engine stops when its stdin closes"
Check ($t.p.ExitCode -eq 0) "graceful exit code 0 ($($t.p.ExitCode))"
Start-Sleep 1
Check (@(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq $tgExe }).Count -eq 0) "no engine process left"
Check ((ServiceState) -eq $svcNow) "helper service untouched by the proxy stop ($svcNow)"
$err = $t.p.StandardError.ReadToEnd()
Check (-not $err.ToLower().Contains($t.secret)) "secret not in the engine log"
# A port in use is reported, the other program keeps it.
$busy = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, $tgPort); $busy.Start()
$t2 = StartTg $tgPort
Check (-not $t2.ready) "busy port: the engine does not take it"
$t2.p.WaitForExit(20000) | Out-Null
Check ($t2.p.ExitCode -eq 3) "busy port reported (exit $($t2.p.ExitCode))"
$busy.Stop()

Write-Host "== 3. a foreign process holds a Flowseal file exclusively"
$held = [IO.File]::Open("$inst\flowseal\$ver\bin\cygwin1.dll", 'Open', 'Read', 'None')
try {
  $code = Run $Setup @('/S')
  Write-Host "  installer exit code: $code"
  Check ($code -eq 0 -or $code -eq 5) "either a clean upgrade or a clear refusal (5), never a skip (exit $code)"
  Check ((ServiceState) -eq 'Running') "helper running after the attempt"
}
finally { $held.Close() }
# Checked after the handle is released (it is exclusive: hashing needs read access).
Check ((BundleIntact) -eq 'ok') "no mixed versions after the attempt ($(BundleIntact))"
$code = Run $Setup @('/S')
Check ($code -eq 0) "the same upgrade succeeds once the file is released (exit $code)"
Check ((BundleIntact) -eq 'ok') "Flowseal files intact ($(BundleIntact))"

Write-Host "== 4. verified stop: ours vs foreign winws"
function StartWinws($dir) {
  New-Item -ItemType Directory -Force $dir | Out-Null
  Copy-Item "$inst\flowseal\$ver\bin\*" $dir -Recurse -Force
  Start-Process "$dir\winws.exe" -ArgumentList '--wf-tcp=9' -WorkingDirectory $dir -PassThru -WindowStyle Hidden
}
$out = & "$inst\discord-bridge-helper.exe" --stop-engine 2>&1
Check ($LASTEXITCODE -eq 0) "--stop-engine exit $LASTEXITCODE ($out)"
Check ((ServiceState) -eq 'Stopped') "service stopped after --stop-engine"
# A foreign winws is neither stopped nor waited for.
$foreign = StartWinws (Join-Path $env:RUNNER_TEMP 'foreign-zapret\bin')
Start-Sleep 3
$out = & "$inst\discord-bridge-helper.exe" --stop-engine 2>&1
Check ($LASTEXITCODE -eq 0) "foreign winws does not block --stop-engine (exit $LASTEXITCODE)"
Check (-not $foreign.HasExited) "foreign winws left running"
Stop-Process -Id $foreign.Id -Force -ErrorAction SilentlyContinue
# A winws from the immutable ProgramData copy is ours: reported, not ignored.
$ours = StartWinws (Join-Path $env:ProgramData 'DiscordBridge\engines\ci-test-copy\bin')
Start-Sleep 3
$out = & "$inst\discord-bridge-helper.exe" --stop-engine 2>&1
Check ($LASTEXITCODE -eq 4) "winws from ProgramData\DiscordBridge\engines is recognised as ours (exit $LASTEXITCODE, $out)"
Stop-Process -Id $ours.Id -Force -ErrorAction SilentlyContinue
Start-Sleep 1
Start-Service DiscordBridgeHelper

Write-Host "== 5. uninstall"
$code = Run "$inst\uninstall.exe" @('/S', "_?=$inst")
Check ($code -eq 0) "uninstall (exit $code)"
Check ((ServiceState) -eq 'absent') "service removed"

if ($Previous) {
  Write-Host "== 6. clean install of the new build after uninstall"
  $code = Run $Setup @('/S')
  Check ($code -eq 0) "clean install (exit $code)"
  Check ((BundleIntact) -eq 'ok') "Flowseal files intact"
  Check ((ServiceState) -eq 'Running') "helper running"
  $code = Run "$inst\uninstall.exe" @('/S', "_?=$inst")
  Check ($code -eq 0) "final uninstall (exit $code)"
}

"### Installer integration" | Out-File -Append $env:GITHUB_STEP_SUMMARY
if ($fails.Count) { $fails | ForEach-Object { "- FAIL $_" } | Out-File -Append $env:GITHUB_STEP_SUMMARY; Write-Host "$($fails.Count) check(s) failed"; exit 1 }
"- all checks passed" | Out-File -Append $env:GITHUB_STEP_SUMMARY
# Explicit result: some checks expect non-zero exit codes from native tools,
# which must not leak into the step's exit code.
$global:LASTEXITCODE = 0
exit 0
