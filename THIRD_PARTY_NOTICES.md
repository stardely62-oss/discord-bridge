# Third-party notices

Bridge (MIT) bundles the following unmodified third-party components
in its Windows installer. Their authorship and licenses are retained.

| Component | Version | Author | License | Source |
|---|---|---|---|---|
| Flowseal `zapret-discord-youtube` — strategy files (`general*.bat`), `lists/`, fake payloads in `bin/*.bin` | 1.10.4 | © 2024-2026 Flowseal | MIT | https://github.com/Flowseal/zapret-discord-youtube |
| `winws.exe` (zapret engine, shipped inside the Flowseal release) | v72.9 | © 2016-2026 bol-van | MIT | https://github.com/bol-van/zapret |
| `WinDivert.dll`, `WinDivert64.sys` (shipped inside the Flowseal release) | 2.x | © basil00 | LGPLv3 **or** GPLv2 (dual) | https://github.com/basil00/WinDivert |
| `cygwin1.dll` (runtime of winws, shipped inside the Flowseal release) | 3.4.x | © Red Hat and contributors | LGPLv3 | https://cygwin.com/ (source: https://cygwin.com/git/) |

## What exactly is shipped

The installer contains a subset of the official Flowseal release archive
`zapret-discord-youtube-1.10.4.zip`, downloaded from
`https://github.com/Flowseal/zapret-discord-youtube/releases/download/1.10.4/`
and verified against the pinned SHA-256
`291af673bd0238506f9548e0e94ca0abf244000f4c9a06960544fb01c5f97b34`
(`resources/flowseal.lock.json`): the strategy `.bat` files (read as data, never
executed), `bin/` and `lists/`. `service.bat` and `utils/` are not shipped.
No file is modified; per-file SHA-256 hashes are listed in
`flowseal/1.10.4/bundle-manifest.json` inside the installation directory.

## License texts

Full license texts are installed in the `licenses/` directory:

- `Flowseal-LICENSE.txt` — MIT (Flowseal, includes bol-van copyright notice)
- `zapret-LICENSE.txt` — MIT (bol-van/zapret)
- `WinDivert-LICENSE.txt` — WinDivert dual license with LGPLv3/GPLv3/GPLv2 texts
- `LGPL-3.0.txt`, `GPL-3.0.txt`, `GPL-2.0.txt` — GNU licenses (Cygwin, WinDivert)

### LGPL components

WinDivert and Cygwin are used as separate, dynamically loaded binaries that
are distributed exactly as published by their authors. Corresponding source
code is available from the upstream projects listed above. You may replace
these files with your own builds; note that Bridge verifies hashes of
engine files before starting them, so replacing them requires a bundle with a
matching manifest.

## Telegram WS Proxy (`tg-ws-proxy/`)

`discord-bridge-tg-ws-proxy.exe` is the network engine of
[Flowseal/tg-ws-proxy](https://github.com/Flowseal/tg-ws-proxy) 1.11.4
(commit `2085861957070eeea07990a522b29f42e5230210`, © 2026 Flowseal, MIT),
packaged headless with PyInstaller by the Bridge CI from that exact
commit. Only the package `proxy` of upstream is included, unmodified; the
tray, GUI, updater and clipboard parts are not. The Bridge runner
(`telegram-proxy/tgproxy_headless.py`, MIT) reads the configuration from
stdin and reports status as JSON lines. Its SHA-256 is listed in
`tg-ws-proxy/tg-ws-proxy.json` and compiled into Bridge.

Bundled inside the executable, each under its own license (texts in
`tg-ws-proxy/licenses/`):

| Component | Version | License |
|---|---|---|
| tg-ws-proxy (`proxy` package) | 1.11.4 | MIT — `tg-ws-proxy/tg-ws-proxy-LICENSE.txt` |
| CPython | 3.11.9 | PSF License |
| cryptography (with OpenSSL) | 46.0.5 | Apache-2.0 or BSD-3-Clause; OpenSSL Apache-2.0 |
| cffi / pycparser | 2.1.1 / 3.12 | MIT / BSD-3-Clause |
| httpx / httpcore / h11 | 0.28.1 / 1.0.9 / 0.16.0 | BSD-3-Clause / BSD-3-Clause / MIT |
| h2 / hpack / hyperframe | 4.4.1 / 4.2.0 / 6.1.0 | MIT |
| anyio / idna / typing-extensions | 4.16.0 / 3.20 / 4.16.0 | MIT / BSD-3-Clause / PSF |
| certifi | 2025.10.5 | MPL-2.0 (unmodified; source: https://github.com/certifi/python-certifi) |
| PyInstaller bootloader | 6.16.0 | GPL-2.0 with the bootloader exception (allows distribution with any program) |

## Rust and JavaScript dependencies

Rust crates (Tauri, Tokio, Serde, windows-sys, reqwest, ed25519-dalek, zip …)
and npm packages (React, Motion, Lucide, Zustand, CodeMirror, Tailwind CSS …)
are used under their respective permissive licenses (MIT, Apache-2.0, ISC,
BSD). See `Cargo.lock` and `pnpm-lock.yaml` for the exact versions.

## Trademarks

Discord is a trademark of Discord Inc. Bridge is an independent
project and is not affiliated with, endorsed or sponsored by Discord Inc.
