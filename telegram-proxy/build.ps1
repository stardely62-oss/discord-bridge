# Builds the headless tg-ws-proxy engine for Discord Bridge (Windows CI).
#   ./telegram-proxy/build.ps1 [-Out resources/tg-ws-proxy]
# - upstream source: git fetch of the pinned commit (verified by its SHA-1);
# - Python packages: --require-hashes, binary wheels only, no dependencies
#   resolved from the index;
# - output: discord-bridge-tg-ws-proxy.exe + tg-ws-proxy.json (version, commit,
#   SHA-256, size) + the upstream MIT license.
param([string]$Out = "resources/tg-ws-proxy")
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$lock = Get-Content (Join-Path $here 'upstream.lock.json') -Raw | ConvertFrom-Json
$tmp = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }

$pyver = (python -c "import sys;print('.'.join(map(str,sys.version_info[:3])))").Trim()
if ($pyver -ne $lock.python) { throw "Python $pyver, expected $($lock.python)" }

$src = Join-Path $tmp 'tg-ws-proxy-src'
if (Test-Path $src) { Remove-Item -Recurse -Force $src }
git init -q $src
git -C $src fetch -q --depth 1 $lock.repository $lock.commit
if ($LASTEXITCODE -ne 0) { throw "git fetch failed" }
git -C $src -c advice.detachedHead=false checkout -q FETCH_HEAD
$head = (git -C $src rev-parse HEAD).Trim()
if ($head -ne $lock.commit) { throw "upstream HEAD $head, expected $($lock.commit)" }
$ver = (Select-String -Path (Join-Path $src 'proxy/__init__.py') -Pattern '__version__ = "([^"]+)"').Matches[0].Groups[1].Value
if ($ver -ne $lock.version) { throw "upstream version $ver, expected $($lock.version)" }

$venv = Join-Path $tmp 'tgproxy-venv'
if (Test-Path $venv) { Remove-Item -Recurse -Force $venv }
python -m venv $venv
$py = Join-Path $venv 'Scripts/python.exe'
& $py -m pip install --disable-pip-version-check --no-input --no-deps --only-binary=:all: --require-hashes `
  -r (Join-Path $here 'requirements-runtime.txt') -r (Join-Path $here 'requirements-build.txt')
if ($LASTEXITCODE -ne 0) { throw "pip install failed" }

$work = Join-Path $tmp 'tgproxy-build'
$env:TGPROXY_UPSTREAM = $src
& $py -m PyInstaller --noconfirm --clean --distpath (Join-Path $work 'dist') --workpath (Join-Path $work 'work') (Join-Path $here 'headless.spec')
if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed" }
$exe = Join-Path $work 'dist/tg-ws-proxy-headless.exe'

# The built engine must load without any GUI/tray module.
$st = (& $exe --self-test) | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or $st.forbiddenLoaded.Count -ne 0) { throw "self-test failed: $($st | ConvertTo-Json -Compress)" }
if ($st.version -ne $lock.version) { throw "engine version $($st.version), expected $($lock.version)" }

New-Item -ItemType Directory -Force $Out | Out-Null
Copy-Item $exe (Join-Path $Out 'discord-bridge-tg-ws-proxy.exe') -Force
Copy-Item (Join-Path $src 'LICENSE') (Join-Path $Out 'tg-ws-proxy-LICENSE.txt') -Force
# License texts of everything inside the executable: Python itself and the
# runtime packages (from their wheel metadata).
$lic = Join-Path $Out 'licenses'
if (Test-Path $lic) { Remove-Item -Recurse -Force $lic }
New-Item -ItemType Directory -Force $lic | Out-Null
$pyroot = (& $py -c "import sys;print(sys.base_prefix)").Trim()
Copy-Item (Join-Path $pyroot 'LICENSE.txt') (Join-Path $lic 'Python-LICENSE.txt')
$site = (& $py -c "import sysconfig;print(sysconfig.get_paths()['purelib'])").Trim()
$runtime = Get-Content (Join-Path $here 'requirements-runtime.txt') | Where-Object { $_ -match '^([A-Za-z0-9_.-]+)==' } | ForEach-Object { $Matches[1] }
foreach ($name in $runtime) {
  $norm = ($name -replace '-', '_').ToLower()
  $di = Get-ChildItem $site -Directory | Where-Object { $_.Name -match '\.dist-info$' -and ($_.Name -split '-')[0].ToLower() -eq $norm } | Select-Object -First 1
  if (-not $di) { throw "no metadata for $name" }
  $files = @(Get-ChildItem $di.FullName -Recurse -File | Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE|AUTHORS)' })
  if ($files.Count -eq 0) { throw "no license file for $name" }
  $dest = Join-Path $lic $name
  New-Item -ItemType Directory -Force $dest | Out-Null
  foreach ($f in $files) { Copy-Item $f.FullName (Join-Path $dest $f.Name) -Force }
}

$sha = (Get-FileHash (Join-Path $Out 'discord-bridge-tg-ws-proxy.exe') -Algorithm SHA256).Hash.ToLower()
$size = (Get-Item (Join-Path $Out 'discord-bridge-tg-ws-proxy.exe')).Length
[ordered]@{
  component = 'tg-ws-proxy-headless'
  version = $lock.version
  upstreamCommit = $lock.commit
  runner = $st.runner
  python = $st.python
  sha256 = $sha
  size = $size
} | ConvertTo-Json | Set-Content -Encoding utf8NoBOM (Join-Path $Out 'tg-ws-proxy.json')
Write-Host "tg-ws-proxy-headless $($lock.version) ($($lock.commit.Substring(0,7))): $size bytes, sha256 $sha"
