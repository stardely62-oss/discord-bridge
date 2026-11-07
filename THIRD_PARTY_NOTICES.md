# Third-party notices

Bridge (formerly Discord Bridge; MIT, see `LICENSE`) ships third-party
components in its Windows installer. Their authorship and licenses are
retained, and none of them is modified by Bridge. This file lists each of them
with its exact version, origin, license, where its license text is installed,
and where its corresponding source code is.

The facts below are checked in CI where possible (see
[docs/third-party-compliance.md](docs/third-party-compliance.md), which also
lists the open questions). Installed paths are relative to the installation
directory, by default `C:\Program Files\Bridge` (or `C:\Program Files\Discord
Bridge` for installations upgraded from 0.1.0-alpha.10 or earlier).

## 1. Components shipped as separate files

| Component | Version | Copyright | License | License text in the installation |
|---|---|---|---|---|
| Flowseal `zapret-discord-youtube` — strategy files (`general*.bat`), `lists/`, fake payloads in `bin/*.bin` | 1.10.4 | © 2016-2026 bol-van, © 2024-2026 Flowseal | MIT | `licenses/Flowseal-LICENSE.txt` |
| `winws.exe` — zapret engine | v72.9 | © 2016-2024 bol-van | MIT | `licenses/zapret-LICENSE.txt` |
| `WinDivert.dll`, `WinDivert64.sys` | 2.2.2 | © Basil (basil00) 2011-2022 | LGPLv3 **or** GPLv2 (dual); Bridge distributes them under **LGPLv3** | `licenses/WinDivert-LICENSE.txt` (with LGPLv3, GPLv3, GPLv2), `licenses/LGPL-3.0.txt`, `licenses/GPL-3.0.txt`, `licenses/GPL-2.0.txt` |
| `cygwin1.dll` — runtime of winws | 3.4.10 (package `cygwin-3.4.10-1`) | © Cygwin Authors 1996-2023 (Red Hat and contributors) | LGPLv3 or later, with the Cygwin Linking Exception | `licenses/Cygwin-LICENSE.txt`, `licenses/LGPL-3.0.txt`, `licenses/GPL-3.0.txt` |
| `tg-ws-proxy/discord-bridge-tg-ws-proxy.exe` — Telegram WS Proxy engine | 1.11.4 | © 2026 Flowseal (and the authors in section 3) | MIT (and section 3) | `tg-ws-proxy/tg-ws-proxy-LICENSE.txt`, `tg-ws-proxy/licenses/` |

### What exactly is shipped from Flowseal

The installer contains a subset of the official Flowseal release archive
`zapret-discord-youtube-1.10.4.zip`, downloaded from
https://github.com/Flowseal/zapret-discord-youtube/releases/download/1.10.4/zapret-discord-youtube-1.10.4.zip
and verified against the pinned SHA-256
`291af673bd0238506f9548e0e94ca0abf244000f4c9a06960544fb01c5f97b34`
(`resources/flowseal.lock.json`; release tag `1.10.4`, commit
`176f56dc3f11ce5915c26abcdde903f93d835b64`): the strategy `.bat` files (read
as data, never executed), `bin/` and `lists/`. `service.bat` and `utils/` are
not shipped. No file is modified; per-file SHA-256 hashes are listed in
`flowseal/1.10.4/bundle-manifest.json` inside the installation directory.

## 2. Origin and corresponding source of the engine binaries

The four engine binaries in `flowseal/1.10.4/bin/` are byte-identical to the
files in `zapret-v72.9/binaries/windows-x86_64/` of the official zapret release
https://github.com/bol-van/zapret/releases/download/v72.9/zapret-v72.9.zip
(also listed in that release's `sha256sum.txt`). The WinDivert files are in
turn byte-identical to the official WinDivert 2.2.2 release. The same data in
machine-readable form: `licenses/third-party-sources.json`.

| File | SHA-256 | Upstream binary | Corresponding source |
|---|---|---|---|
| `winws.exe` | `affb4f69d2ea302a7abccd5325d81826e140ddae014f1e070bc4a6c0dd555188` | zapret v72.9 | https://github.com/bol-van/zapret, tag `v72.9`, commit `c849e55ef0f1c244206f5a05ff7b1ab41a3824ee` — archive https://github.com/bol-van/zapret/archive/refs/tags/v72.9.tar.gz (winws: `nfq/`, Windows build: `docs/windows.md`) |
| `WinDivert.dll` | `c1e060ee19444a259b2162f8af0f3fe8c4428a1c6f694dce20de194ac8d7d9a2` | https://github.com/basil00/WinDivert/releases/download/v2.2.2/WinDivert-2.2.2-A.zip (`x64/WinDivert.dll`) | https://github.com/basil00/WinDivert, tag `v2.2.2`, commit `1789526ecfb9ff5397c94f9f54c1a3dc2fb60440` — archive https://github.com/basil00/WinDivert/archive/refs/tags/v2.2.2.tar.gz (build scripts: `msvc-build.bat`, `wddk-build.bat`, `mingw-build.sh`) |
| `WinDivert64.sys` | `8da085332782708d8767bcace5327a6ec7283c17cfb85e40b03cd2323a90ddc2` | same archive (`x64/WinDivert64.sys`), with its original signature | same source as `WinDivert.dll` |
| `cygwin1.dll` | `103104a52e5293ce418944725df19e2bf81ad9269b9a120d71d39028e821499b` | official Cygwin build of package `cygwin-3.4.10-1` (x86_64), as shipped in zapret v72.9 | https://cygwin.com/cgit/newlib-cygwin/, tag `cygwin-3.4.10`, commit `89a1e4a573d40b233011402f42630b970ff71291` — mirror https://github.com/cygwin/cygwin/tree/cygwin-3.4.10, archive https://github.com/cygwin/cygwin/archive/refs/tags/cygwin-3.4.10.tar.gz |

**Copy kept by this project.** Exact archives of these three sources (the
upstream tag archives, unmodified, with SHA-256) are published in the release
[`sources-flowseal-1.10.4`](https://github.com/stardely62-oss/discord-bridge/releases/tag/sources-flowseal-1.10.4) of this repository:
`WinDivert-2.2.2-source.tar.gz`, `cygwin-3.4.10-source.tar.gz`,
`zapret-v72.9-source.tar.gz`.

`winws.exe` loads `cygwin1.dll` and `WinDivert.dll` through its Windows import
table (load-time dynamic linking) from its own directory; `WinDivert.dll`
installs and opens the `WinDivert64.sys` driver from the same directory. zlib
is linked statically into `winws.exe` (zlib license). Bridge does not link
any of these files: its helper service starts `winws.exe` as a separate
process with arguments taken from the Flowseal strategy files.

### Replacing LGPL components

WinDivert (LGPLv3) and the Cygwin DLL (LGPLv3 or later) may be replaced with
your own builds of a modified, interface-compatible version. Bridge verifies
engine files against the bundle manifest before every start, so that a file
changed silently is never run with administrator rights; a replacement made
deliberately by an administrator is accepted:

1. Build the library from its corresponding source (table above).
2. In Bridge, turn the bypass off (the helper keeps the engine files locked
   while it runs them).
3. As an administrator, replace the file in `flowseal\1.10.4\bin\` of the
   installation directory.
4. In `flowseal\1.10.4\bundle-manifest.json`, set `sha256` and `size` of
   that file's entry to the values of your build (for example
   `Get-FileHash <file> -Algorithm SHA256`).
5. Turn the bypass on again.

Notes:
- 64-bit Windows loads only kernel drivers signed in a way Microsoft accepts.
  A self-built `WinDivert64.sys` therefore needs your own signature or
  Windows test-signing mode. This is a Windows requirement, not a Bridge one.
- The helper runs winws from a verified copy in
  `C:\ProgramData\DiscordBridge\engines\<version>-<manifest hash>`; a changed
  manifest yields a new copy, so the replacement takes effect at the next start.
- Reinstalling or updating Bridge restores the original files. Flowseal
  updates installed by Bridge (`C:\ProgramData\DiscordBridge\bundles`) can be
  handled the same way.
- `winws.exe` itself is MIT; it can be replaced the same way.

## 3. Telegram WS Proxy engine (`tg-ws-proxy/`)

`discord-bridge-tg-ws-proxy.exe` is the network engine of
[Flowseal/tg-ws-proxy](https://github.com/Flowseal/tg-ws-proxy) 1.11.4
(commit `2085861957070eeea07990a522b29f42e5230210`, © 2026 Flowseal, MIT),
packaged headless with PyInstaller by the Bridge CI from that exact
commit. Only the package `proxy` of upstream is included, unmodified; the
tray, GUI, updater and clipboard parts are not. The Bridge runner
(`telegram-proxy/tgproxy_headless.py`, MIT) reads the configuration from
stdin and reports status as JSON lines. Its SHA-256 is listed in
`tg-ws-proxy/tg-ws-proxy.json` and compiled into Bridge. The build
(`telegram-proxy/build.ps1`, `headless.spec`, hash-locked
`requirements-*.txt`) is in this repository.

Bundled inside the executable, each under its own license (texts in
`tg-ws-proxy/licenses/`, collected from each package's metadata at build time):

| Component | Version | License | Source |
|---|---|---|---|
| tg-ws-proxy (`proxy` package) | 1.11.4 | MIT — `tg-ws-proxy/tg-ws-proxy-LICENSE.txt` | https://github.com/Flowseal/tg-ws-proxy/tree/2085861957070eeea07990a522b29f42e5230210 |
| CPython (python311.dll, standard library; Windows build) | 3.11.9 | PSF License; the Windows build also includes OpenSSL 3 (`libssl-3.dll`, `libcrypto-3.dll`, Apache-2.0), libffi (MIT) and Microsoft Distributable Code (`vcruntime140*.dll`) under the conditions stated in `Python-LICENSE.txt` | https://www.python.org/downloads/release/python-3119/ |
| cryptography (with a statically linked OpenSSL) | 46.0.5 | Apache-2.0 or BSD-3-Clause; OpenSSL Apache-2.0 | https://pypi.org/project/cryptography/46.0.5/ |
| cffi / pycparser | 2.1.1 / 3.12 | MIT / BSD-3-Clause | PyPI |
| httpx / httpcore / h11 | 0.28.1 / 1.0.9 / 0.16.0 | BSD-3-Clause / BSD-3-Clause / MIT | PyPI |
| h2 / hpack / hyperframe | 4.4.1 / 4.2.0 / 6.1.0 | MIT | PyPI |
| anyio / idna / typing-extensions | 4.16.0 / 3.20 / 4.16.0 | MIT / BSD-3-Clause / PSF | PyPI |
| certifi (CA bundle, included unmodified as `cacert.pem`) | 2025.10.5 | MPL-2.0 | https://pypi.org/project/certifi/2025.10.5/ (source form: https://github.com/certifi/python-certifi/tree/2025.10.05) |
| PyInstaller bootloader | 6.16.0 | GPL-2.0 with the bootloader exception (it allows distributing the bootloader with any program, without GPL obligations for that program) — `tg-ws-proxy/licenses/pyinstaller/` | https://github.com/pyinstaller/pyinstaller/tree/v6.16.0 |

The engine executable is checked against the SHA-256 compiled into Bridge;
none of the components above is under the LGPL or GPL without an exception,
so no relinking provision applies to it.

## 4. Rust crates and npm packages compiled into the app

`discord-bridge.exe` and `discord-bridge-helper.exe` contain Rust crates
(Tauri, Tokio, Serde, windows-sys, reqwest, ed25519-dalek, zip …), and the
interface embedded in `discord-bridge.exe` contains npm packages (React,
Motion, Lucide, Zustand, CodeMirror, Tailwind CSS …). Their license notices
are collected from the packages at build time by
`scripts/third-party-licenses.mjs` and installed as:

- `licenses/third-party/THIRD_PARTY_LICENSES.txt` — the license and notice
  files of every crate and npm package compiled in, verbatim;
- `licenses/third-party/packages.json` — name, version, license and source of
  each of them.

The build fails if any of them has no license or a license outside a list of
permissive ones (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode, MPL-2.0 and similar;
see the script). The MPL-2.0 crates (`cssparser`, `selectors`, `dtoa-short`,
`option-ext` and others listed there) are unmodified; their source is published
on crates.io at the exact version listed. Exact versions: `Cargo.lock`,
`pnpm-lock.yaml`.

## 5. Trademarks

Discord is a trademark of Discord Inc. Bridge is an independent
project and is not affiliated with, endorsed or sponsored by Discord Inc.
Telegram, YouTube, Google and Cloudflare are trademarks of their owners.
