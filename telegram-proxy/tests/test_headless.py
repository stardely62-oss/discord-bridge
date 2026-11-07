"""Tests of the headless runner against the pinned upstream engine.

Run with the upstream checkout on PYTHONPATH (see telegram-proxy/README.md):
    PYTHONPATH=<tg-ws-proxy checkout> python -m pytest telegram-proxy/tests
"""
import json
import logging
import os
import socket
import subprocess
import sys
import time
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(HERE))
import tgproxy_headless as h  # noqa: E402

_HEX16 = "0123456789abcdef"
SECRET = _HEX16 * 2  # test value built at runtime, not a real secret


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def cfg(**over):
    c = {"host": "127.0.0.1", "port": free_port(), "secret": SECRET,
         "fallbackCfproxy": False, "poolSize": 0}
    c.update(over)
    return c


def spawn(config, extra_env=None):
    env = dict(os.environ)
    env.update(extra_env or {})
    p = subprocess.Popen(
        [sys.executable, str(HERE / "tgproxy_headless.py"), "--stdin-config"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, env=env,
    )
    p.stdin.write(json.dumps(config) + "\n")
    p.stdin.flush()
    return p


def read_event(p, name, timeout=20):
    deadline = time.time() + timeout
    while time.time() < deadline:
        line = p.stdout.readline()
        if not line:
            break
        ev = json.loads(line)
        if ev["event"] == name:
            return ev
        if ev["event"] == "error":
            raise AssertionError(f"unexpected error event: {ev}")
    raise AssertionError(f"no {name} event")


# ---------------------------------------------------------------- config

def test_valid_config_is_accepted_with_safe_defaults():
    c = h.parse_config(json.dumps({"secret": SECRET}))
    assert c["host"] == "127.0.0.1" and c["port"] == 1443
    assert c["fallback_cfproxy"] is False, "no third-party relay unless chosen"
    assert c["cfproxy_domains"] == [] and c["cfproxy_worker_domains"] == []


@pytest.mark.parametrize("bad", [
    {"secret": SECRET, "host": "0.0.0.0"},
    {"secret": SECRET, "host": "192.168.1.5"},
    {"secret": SECRET, "port": 80},
    {"secret": SECRET, "port": 70000},
    {"secret": SECRET, "port": "1443"},
    {"secret": "dd" + SECRET},
    {"secret": SECRET.upper()},
    {"secret": SECRET[:30]},
    {"secret": SECRET, "unexpected": 1},
    {"secret": SECRET, "cfproxyDomains": ["not a domain"]},
    {"secret": SECRET, "poolSize": 100},
])
def test_invalid_config_is_refused(bad):
    with pytest.raises(h.ConfigError):
        h.parse_config(json.dumps(bad))


def test_user_domains_are_normalised_by_the_upstream_rules():
    c = h.parse_config(json.dumps({"secret": SECRET, "fallbackCfproxy": True,
                                   "cfproxyDomains": ["Example.COM"]}))
    assert c["cfproxy_domains"] == ["example.com"]


def test_secret_is_redacted_from_log_records():
    f = h.SecretRedactFilter(SECRET)
    rec = logging.LogRecord("x", logging.INFO, __file__, 1,
                            "link tg://proxy?secret=dd%s and %s", (SECRET, SECRET.upper()), None)
    f.filter(rec)
    assert SECRET not in rec.getMessage().lower()
    assert "<secret>" in rec.getMessage()


# ---------------------------------------------------------------- process

def test_listens_counts_clients_never_prints_the_secret_and_stops_on_eof():
    c = cfg()
    p = spawn(c)
    try:
        ready = read_event(p, "ready")
        assert (ready["host"], ready["port"]) == ("127.0.0.1", c["port"])
        assert ready["version"] == "1.11.4"
        # A client with a wrong handshake: counted, rejected.
        with socket.create_connection(("127.0.0.1", c["port"]), timeout=5) as s:
            s.sendall(os.urandom(64))
            time.sleep(0.5)
        deadline = time.time() + 15
        st = None
        while time.time() < deadline:
            st = read_event(p, "status")
            if st["stats"]["connections_total"] >= 1:
                break
        assert st and st["listening"] is True
        assert st["stats"]["connections_total"] >= 1
        assert st["stats"]["bytes_down"] == 0, "nothing reached Telegram"
    finally:
        p.stdin.close()  # the parent goes away
        p.wait(timeout=30)
        out, err = p.stdout.read(), p.stderr.read()
    assert p.returncode == 0
    assert '"event":"stopped"' in out
    assert SECRET not in out and SECRET not in err
    assert "<secret>" in err, "the engine's startup banner was redacted"


def test_stop_command_stops_gracefully():
    p = spawn(cfg())
    read_event(p, "ready")
    p.stdin.write("stop\n")
    p.stdin.flush()
    assert p.wait(timeout=30) == 0


def test_address_in_use_is_reported_and_the_other_listener_is_untouched():
    with socket.socket() as busy:
        busy.bind(("127.0.0.1", 0))
        busy.listen()
        port = busy.getsockname()[1]
        p = spawn(cfg(port=port))
        out, err = p.communicate(timeout=30)
        assert p.returncode == 3
        ev = json.loads(out.strip().splitlines()[-1])
        assert ev == {"event": "error", "code": "addressInUse", "message": ev["message"]}
        assert SECRET not in out + err
        # The foreign listener still works.
        with socket.create_connection(("127.0.0.1", port), timeout=5):
            pass


def test_bad_config_exits_2_without_echoing_it():
    p = subprocess.run(
        [sys.executable, str(HERE / "tgproxy_headless.py"), "--stdin-config"],
        input=json.dumps({"secret": "zz" + SECRET[2:]}) + "\n",
        capture_output=True, text=True, timeout=30,
    )
    assert p.returncode == 2
    assert SECRET[2:] not in p.stdout + p.stderr


def test_self_test_loads_no_gui_or_tray_module():
    p = subprocess.run([sys.executable, str(HERE / "tgproxy_headless.py"), "--self-test"],
                       capture_output=True, text=True, timeout=60)
    ev = json.loads(p.stdout)
    assert p.returncode == 0, ev
    assert ev["forbiddenLoaded"] == []
    assert ev["version"] == "1.11.4"


def test_only_known_arguments_are_accepted():
    p = subprocess.run([sys.executable, str(HERE / "tgproxy_headless.py"), "--secret", SECRET],
                       capture_output=True, text=True, timeout=30)
    assert p.returncode == 2


def test_without_the_fallback_no_relay_list_is_downloaded():
    p = spawn(cfg())
    read_event(p, "ready")
    time.sleep(1.5)
    p.stdin.close()
    p.wait(timeout=30)
    err = p.stderr.read()
    assert "CF proxy" not in err and "cfproxy" not in err.lower(), err


def test_stop_right_after_start_is_quick_and_graceful():
    for _ in range(3):
        p = spawn(cfg(poolSize=4))  # the engine warms up WebSocket connections
        read_event(p, "ready")
        t = time.time()
        p.stdin.close()
        assert p.wait(timeout=15) == 0
        assert time.time() - t < 8, "stop waited for the warm-up"
        assert '"event":"stopped"' in p.stdout.read()
