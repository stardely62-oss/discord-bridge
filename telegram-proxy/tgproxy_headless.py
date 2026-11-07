"""Headless runner of Flowseal tg-ws-proxy for Discord Bridge.

Runs the unmodified network engine of tg-ws-proxy (package ``proxy`` of the
pinned upstream commit, see UPSTREAM.md) without the tray, GUI, updater or
clipboard code of the official Windows build. Discord Bridge (Rust) owns the
process:

* configuration, including the MTProto secret, arrives as ONE JSON line on
  stdin — never on the command line, never in a file written by this process;
* the process stops when stdin is closed or receives ``stop`` (the parent
  exits or stops the proxy); the parent also holds it in a Job Object;
* stdout carries machine-readable JSON lines (``ready``, ``status``,
  ``error``, ``stopped``) without the secret;
* stderr carries the engine log; the secret is redacted from every record.

Exit codes: 0 stopped on request, 2 bad configuration, 3 address in use,
4 listen failed for another reason, 1 unexpected error.
"""

from __future__ import annotations

import asyncio
import errno
import json
import logging
import os
import re
import sys
import threading
import time
from typing import Any, Dict, List, Optional

RUNNER_VERSION = "1"
STATUS_INTERVAL = 2.0
STOP_GRACE = 2.0

# Modules of the upstream GUI/tray build that must never be loaded here.
FORBIDDEN_MODULES = (
    "tkinter", "customtkinter", "pystray", "PIL", "pyperclip", "psutil",
    "utils.tray_common", "windows", "ui",
)

_HEX32 = re.compile(r"^[0-9a-f]{32}$")
_LOOPBACK = {"127.0.0.1"}


class ConfigError(ValueError):
    pass


def _domain_list(value: Any, field: str) -> List[str]:
    if value is None:
        return []
    if not isinstance(value, list) or not all(isinstance(v, str) for v in value):
        raise ConfigError(f"{field} must be a list of strings")
    from proxy.config import _normalize_domain_pool  # upstream validation
    items = _normalize_domain_pool([v.strip() for v in value if v.strip()])
    if len(items) != len([v for v in value if v.strip()]):
        raise ConfigError(f"{field} contains an invalid domain")
    if len(items) > 32:
        raise ConfigError(f"{field} has too many domains")
    return items


def parse_config(line: str) -> Dict[str, Any]:
    """Validate the JSON configuration sent by Discord Bridge."""
    try:
        raw = json.loads(line)
    except ValueError as exc:
        raise ConfigError("configuration is not JSON") from exc
    if not isinstance(raw, dict):
        raise ConfigError("configuration must be an object")
    known = {"host", "port", "secret", "fallbackCfproxy", "h2Media",
             "cfproxyDomains", "cfproxyWorkerDomains", "poolSize", "bufKb",
             "dcIp", "verbose"}
    unknown = set(raw) - known
    if unknown:
        raise ConfigError("unknown configuration keys: " + ", ".join(sorted(unknown)))
    host = raw.get("host", "127.0.0.1")
    if host not in _LOOPBACK:
        raise ConfigError("only 127.0.0.1 is allowed as listen address")
    port = raw.get("port", 1443)
    if not isinstance(port, int) or isinstance(port, bool) or not 1024 <= port <= 65535:
        raise ConfigError("port must be 1024-65535")
    secret = raw.get("secret")
    if not isinstance(secret, str) or not _HEX32.match(secret):
        raise ConfigError("secret must be 32 lowercase hex characters")
    cfg = {
        "host": host,
        "port": port,
        "secret": secret,
        "fallback_cfproxy": bool(raw.get("fallbackCfproxy", False)),
        "h2_media": bool(raw.get("h2Media", True)),
        "cfproxy_domains": _domain_list(raw.get("cfproxyDomains"), "cfproxyDomains"),
        "cfproxy_worker_domains": _domain_list(raw.get("cfproxyWorkerDomains"), "cfproxyWorkerDomains"),
        "pool_size": raw.get("poolSize", 4),
        "buf_kb": raw.get("bufKb", 256),
        "dc_ip": raw.get("dcIp", ["2:149.154.167.220", "4:149.154.167.220"]),
        "verbose": bool(raw.get("verbose", False)),
    }
    for key, lo, hi in (("pool_size", 0, 16), ("buf_kb", 4, 4096)):
        v = cfg[key]
        if not isinstance(v, int) or isinstance(v, bool) or not lo <= v <= hi:
            raise ConfigError(f"{key} must be {lo}-{hi}")
    if not isinstance(cfg["dc_ip"], list) or not all(isinstance(x, str) for x in cfg["dc_ip"]):
        raise ConfigError("dcIp must be a list of DC:IP strings")
    return cfg


class SecretRedactFilter(logging.Filter):
    """Removes the secret (and the dd/ee-prefixed form) from every record."""

    def __init__(self, secret: str):
        super().__init__()
        self._pattern = re.compile(re.escape(secret), re.IGNORECASE)

    def filter(self, record: logging.LogRecord) -> bool:
        msg = record.getMessage()
        red = self._pattern.sub("<secret>", msg)
        if red != msg:
            record.msg, record.args = red, None
        return True


def emit(event: str, **fields: Any) -> None:
    line = json.dumps({"event": event, **fields}, separators=(",", ":"))
    sys.stdout.write(line + "\n")
    sys.stdout.flush()


def stats_snapshot() -> Dict[str, int]:
    from proxy.stats import stats
    keys = ("connections_total", "connections_active", "connections_ws",
            "connections_h2", "connections_tcp_fallback", "connections_cfproxy",
            "connections_fronting", "connections_bad", "connections_masked",
            "ws_errors", "bytes_up", "bytes_down")
    return {k: int(getattr(stats, k, 0)) for k in keys}


def listening() -> bool:
    import proxy.tg_ws_proxy as tgp
    server = tgp._server_instance
    socks = getattr(server, "sockets", None) or ()
    return any(s.fileno() >= 0 for s in socks)


class _LoopGate:
    """The stdin thread may reach the event loop only while it runs."""

    def __init__(self, loop: asyncio.AbstractEventLoop, stop: asyncio.Event):
        self._lock = threading.Lock()
        self._loop: Optional[asyncio.AbstractEventLoop] = loop
        self._stop = stop

    def request_stop(self) -> None:
        with self._lock:
            if self._loop is not None:
                try:
                    self._loop.call_soon_threadsafe(self._stop.set)
                except RuntimeError:
                    pass

    def close(self) -> None:
        with self._lock:
            self._loop = None


def _watch_stdin(gate: _LoopGate) -> None:
    """Stop on EOF (the parent closed the pipe or died) or on "stop"."""
    try:
        for line in sys.stdin:
            if line.strip() == "stop":
                break
    except Exception:
        pass
    gate.request_stop()


def apply_config(cfg: Dict[str, Any]) -> None:
    from proxy.config import parse_dc_ip_list, proxy_config
    proxy_config.host = cfg["host"]
    proxy_config.port = cfg["port"]
    proxy_config.secret = cfg["secret"]
    proxy_config.dc_redirects = parse_dc_ip_list(cfg["dc_ip"])
    proxy_config.buffer_size = cfg["buf_kb"] * 1024
    proxy_config.pool_size = cfg["pool_size"]
    proxy_config.fallback_cfproxy = cfg["fallback_cfproxy"]
    proxy_config.cfproxy_user_domains = cfg["cfproxy_domains"]
    proxy_config.cfproxy_worker_domains = cfg["cfproxy_worker_domains"]
    proxy_config.cfproxy_h2_media = cfg["h2_media"]
    proxy_config.disable_secure = False
    proxy_config.fake_tls_domain = ""
    proxy_config.proxy_protocol = False
    proxy_config.force_test_dc = False
    if not cfg["fallback_cfproxy"]:
        # Upstream starts its relay-list refresh (an hourly download of the
        # Cloudflare relay list from GitHub) whenever no own domains are set,
        # even with the fallback off. Without the fallback the list is never
        # used, so the download is not made.
        import proxy.tg_ws_proxy as tgp
        tgp.start_cfproxy_domain_refresh = lambda: None


def setup_logging(cfg: Dict[str, Any]) -> None:
    from proxy.utils import DomainCensorFilter
    root = logging.getLogger()
    for h in list(root.handlers):
        root.removeHandler(h)
    root.setLevel(logging.DEBUG if cfg["verbose"] else logging.INFO)
    handler = logging.StreamHandler(sys.stderr)
    handler.setFormatter(logging.Formatter("%(asctime)s  %(levelname)-5s  %(message)s", "%H:%M:%S"))
    handler.addFilter(SecretRedactFilter(cfg["secret"]))
    handler.addFilter(DomainCensorFilter())
    root.addHandler(handler)
    for name in ("asyncio", "httpx", "httpcore", "hpack", "h2"):
        logging.getLogger(name).setLevel(logging.WARNING)


async def serve(cfg: Dict[str, Any], stop: Optional[asyncio.Event] = None) -> None:
    import proxy.tg_ws_proxy as tgp
    from proxy import __version__
    loop = asyncio.get_running_loop()
    stop = stop or asyncio.Event()
    gate = _LoopGate(loop, stop)
    threading.Thread(target=_watch_stdin, args=(gate,), daemon=True, name="stdin").start()

    async def status_loop() -> None:
        announced = False
        while True:
            up = listening()
            if up and not announced:
                announced = True
                emit("ready", host=cfg["host"], port=cfg["port"], version=__version__,
                     runner=RUNNER_VERSION, fallbackCfproxy=cfg["fallback_cfproxy"])
            emit("status", listening=up, at=int(time.time()), stats=stats_snapshot())
            await asyncio.sleep(STATUS_INTERVAL)

    status = asyncio.create_task(status_loop())
    run = asyncio.create_task(tgp._run(stop))
    stop_wait = asyncio.create_task(stop.wait())
    try:
        await asyncio.wait({run, stop_wait}, return_when=asyncio.FIRST_COMPLETED)
        if not run.done():
            # The engine honours the stop event only between its start-up
            # steps (e.g. after the WebSocket pool warm-up); do not wait for
            # them: give it a moment to close cleanly, then cancel it.
            try:
                await asyncio.wait_for(asyncio.shield(run), STOP_GRACE)
            except asyncio.TimeoutError:
                run.cancel()
                try:
                    await run
                except asyncio.CancelledError:
                    pass
                return
        run.result()  # re-raise a start failure (e.g. address in use)
    finally:
        stop_wait.cancel()
        gate.close()
        status.cancel()
        try:
            await status
        except asyncio.CancelledError:
            pass


def self_test() -> int:
    import proxy.tg_ws_proxy  # noqa: F401  (the engine imports cleanly)
    from proxy import __version__
    loaded = sorted(m for m in FORBIDDEN_MODULES if m in sys.modules)
    emit("selfTest", version=__version__, runner=RUNNER_VERSION, forbiddenLoaded=loaded,
         python=sys.version.split()[0])
    return 0 if not loaded else 1


def main(argv: Optional[List[str]] = None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    if argv == ["--self-test"]:
        return self_test()
    if argv == ["--version"]:
        from proxy import __version__
        print(__version__)
        return 0
    if argv != ["--stdin-config"]:
        sys.stderr.write("usage: tg-ws-proxy-headless --stdin-config | --self-test | --version\n")
        return 2
    return run_proxy_mode()


def run_proxy_mode() -> int:
    try:
        cfg = parse_config(sys.stdin.readline())
    except ConfigError as exc:
        emit("error", code="config", message=str(exc))
        return 2
    setup_logging(cfg)
    apply_config(cfg)
    try:
        asyncio.run(serve(cfg))
    except OSError as exc:
        in_use = exc.errno in (errno.EADDRINUSE, 10048) or getattr(exc, "winerror", None) == 10048
        emit("error", code="addressInUse" if in_use else "listen",
             message=f"{type(exc).__name__}: errno {exc.errno}")
        return 3 if in_use else 4
    except Exception as exc:  # noqa: BLE001 — reported, never the secret
        emit("error", code="crash", message=type(exc).__name__)
        logging.getLogger("tg-mtproto-proxy").exception("engine stopped with an error")
        return 1
    emit("stopped", stats=stats_snapshot())
    return 0


def _hard_exit(code: int) -> None:
    """End the process without interpreter finalisation: the stdin thread may
    still be blocked in a read, and finalising around it is unsafe."""
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.flush()
        except Exception:
            pass
    logging.shutdown()
    os._exit(code)


if __name__ == "__main__":
    code = main()
    if sys.argv[1:] == ["--stdin-config"]:
        _hard_exit(code)
    sys.exit(code)
