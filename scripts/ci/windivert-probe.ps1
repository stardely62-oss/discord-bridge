# Facts about WinDivert64.sys locking on a real Windows kernel (CI runner).
# Runs winws from a private copy of the Flowseal bin directory with a filter
# that only matches TCP port 9 (discard), then records:
#  - whether the .sys can be opened for writing / renamed while winws runs;
#  - whether the WinDivert driver unloads and frees the file after winws exits;
#  - whether the containing directory can be renamed in each state.
# Informational: the job prints a summary and never fails the build on facts.
param([string]$Bin = "resources/flowseal/1.10.4/bin")
$ErrorActionPreference = 'Continue'
$root = Join-Path $env:RUNNER_TEMP "wd-probe"
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$root\v1" | Out-Null
Copy-Item "$Bin\*" "$root\v1\" -Recurse
$sys = "$root\v1\WinDivert64.sys"
$facts = [ordered]@{}

function OpenWrite($p) {
  try { $f = [IO.File]::Open($p, 'Open', 'ReadWrite', 'None'); $f.Close(); 'writable' }
  catch { 'locked (' + ($_.Exception.InnerException ?? $_.Exception).Message.Trim() + ')' }
}
function Driver { ((sc.exe query WinDivert) | Select-String 'STATE|FAILED') -join ' ' }
function TryRename($from, $to) {
  try { Rename-Item $from $to -ErrorAction Stop; Rename-Item (Join-Path (Split-Path $from) $to) (Split-Path $from -Leaf) -ErrorAction Stop; 'ok' }
  catch { 'failed (' + $_.Exception.Message.Trim() + ')' }
}

$facts['driver before'] = Driver
$facts['sys before'] = OpenWrite $sys
$p = Start-Process "$root\v1\winws.exe" -ArgumentList '--wf-tcp=9' -WorkingDirectory "$root\v1" -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 5
$facts['winws running'] = -not $p.HasExited
$facts['driver while running'] = Driver
$facts['sys open-for-write while running'] = OpenWrite $sys
$facts['sys rename while running'] = TryRename $sys 'WinDivert64.sys.old'
$facts['dir rename while running'] = TryRename "$root\v1" 'v1-old'
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
$p.WaitForExit(10000) | Out-Null
Start-Sleep -Seconds 2
$facts['driver after exit (2s)'] = Driver
$facts['sys open-for-write after exit (2s)'] = OpenWrite $sys
Start-Sleep -Seconds 10
$facts['driver after exit (12s)'] = Driver
$facts['sys open-for-write after exit (12s)'] = OpenWrite $sys
$facts['sys rename after exit'] = TryRename $sys 'WinDivert64.sys.old'
$facts['dir rename after exit'] = TryRename "$root\v1" 'v1-old'

"### WinDivert lock probe" | Tee-Object -FilePath $env:GITHUB_STEP_SUMMARY -Append
foreach ($k in $facts.Keys) { "- **$k**: $($facts[$k])" | Tee-Object -FilePath $env:GITHUB_STEP_SUMMARY -Append }
