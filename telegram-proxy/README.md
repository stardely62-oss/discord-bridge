# Telegram WS Proxy (headless tg-ws-proxy)

Discord Bridge runs the network engine of
[Flowseal/tg-ws-proxy](https://github.com/Flowseal/tg-ws-proxy) as a local
MTProto proxy for Telegram Desktop. It is a separate component from Zapret.

## Origin

| | |
|---|---|
| Upstream | `Flowseal/tg-ws-proxy`, tag `v1.11.4` |
| Commit | `2085861957070eeea07990a522b29f42e5230210` (`upstream.lock.json`) |
| License | MIT, © 2026 Flowseal (shipped as `tg-ws-proxy-LICENSE.txt`) |
| What is used | the package `proxy/`, unmodified |
| What is not | `windows.py`, `macos.py`, `linux.py`, `ui/`, `utils/` (tray, GUI, updater, clipboard, autostart) |

The official Windows build is a GUI program with its own tray and updater.
That build is not used here: Discord Bridge has one tray, and updates of this
component go through Discord Bridge.

## How it runs

```
Discord Bridge (Rust, bridge_telegram)            discord-bridge-tg-ws-proxy.exe
  verify SHA-256 (compiled in + manifest) ──┐
  CreateProcess(suspended, no window) ──────┼──▶ PyInstaller bootloader ─▶ Python
  Job Object (kill on close), resume ───────┘        tgproxy_headless.py
  stdin: {"host":"127.0.0.1","port":…,"secret":…}  ─▶ proxy.tg_ws_proxy._run(stop)
  stdout ◀─ {"event":"ready"|"status"|"error"|"stopped", …}  (no secret)
  stderr ◀─ engine log, secret redacted (and redacted again in Rust)
  stop: "stop" + stdin EOF → graceful; after 6 s the job is terminated
```

The runner (`tgproxy_headless.py`) is the only adaptation:
- it reads the configuration, including the secret, from stdin, never from
  the command line;
- it accepts only `127.0.0.1` and ports 1024–65535;
- it has the relay fallback off unless chosen;
- it redacts the secret from the log;
- it reports the engine counters as JSON;
- it stops when stdin closes, and the parent process then terminates the job.

It calls the same `_run(stop_event)` entry point that the upstream tray uses.
Three behaviours differ from the upstream program, all covered by tests:
- with the relay fallback off, the hourly download of the Cloudflare relay list
  from GitHub is not started. Upstream starts it even with the fallback off,
  although the list is then unused;
- a stop that arrives during the engine's start-up steps (WebSocket pool
  warm-up) is not delayed by them: after 2 s the engine task is cancelled and
  its own cleanup runs;
- the process exits without interpreter finalisation, after flushing its
  output. Finalising while the stdin thread is blocked in a read crashed
  CPython in about 1 of 5 fast stops in testing.

## Build (Windows CI)

`build.ps1`, run by `ci.yml` and `release.yml` before the app is built:

1. Python 3.11.9 (`actions/setup-python`, pinned by SHA).
2. `git fetch` of the pinned commit; HEAD and `proxy/__init__.py` version checked.
3. `pip install --require-hashes --only-binary=:all: --no-deps` of
   `requirements-runtime.txt` + `requirements-build.txt` (every wheel pinned
   by SHA-256; no dependency resolution from the index).
4. PyInstaller one-file console executable (`headless.spec`); GUI modules
   excluded. Discord Bridge starts it with `CREATE_NO_WINDOW`.
5. `--self-test`: the engine imports, and no GUI/tray module
   (`tkinter`, `customtkinter`, `pystray`, `PIL`, `pyperclip`, `psutil`, `ui`,
   `utils.tray_common`) is loaded.
6. Output in `resources/tg-ws-proxy/`:
   - `discord-bridge-tg-ws-proxy.exe`;
   - `tg-ws-proxy.json` — version, commit, SHA-256, size;
   - the license texts of upstream, Python and every runtime package.

`src-tauri/build.rs` compiles the executable's SHA-256 into Discord Bridge.
The app starts only that exact file, and checks it again before every start.
A local build without step 6 reports the component as not installed. Run
`./telegram-proxy/build.ps1` on Windows before `pnpm tauri build`.

PyInstaller builds are not bit-for-bit reproducible. The executable shipped is
the one built in the release workflow; its hash is in the installer
(`tg-ws-proxy.json`) and in `discord-bridge.exe`.

## Tests

- `test-python.sh` (Linux CI):
  - the upstream engine tests at the pinned commit — the GUI, tray, updater and
    `psutil`-dependent tests are excluded, those parts are not shipped;
  - `tests/test_headless.py`: configuration, redaction, stdin stop, busy port,
    self-test.
- `crates/bridge-telegram`:
  - lifecycle with a stand-in engine;
  - `tests/real_engine.rs` with the built executable on Windows CI.
- `scripts/ci/installer-integration.ps1` — from Program Files:
  - integrity of the installed engine;
  - start and graceful stop;
  - no window;
  - independence from the helper and from Zapret;
  - busy port.

## Updates

There is no separate auto-update yet. A new version means:
1. bump `upstream.lock.json`;
2. review the upstream diff;
3. run the tests;
4. release Discord Bridge.

A later separate channel would need its own signing key (not the Flowseal or
catalog keys) and the same verification before start.
