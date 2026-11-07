"""Current authorised connections vs. upstream counters, on the real
upstream `_handle_client` with real obfuscated MTProto handshakes.

Only the far side (Telegram) is replaced: the WebSocket pool returns a stub
and the bridging coroutine waits until the test lets the client go — what
matters here is which connections get relayed, not the bytes.
"""
import asyncio
import hashlib
import os
import struct
import sys
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import tgproxy_headless as h  # noqa: E402
import proxy.tg_ws_proxy as tgp  # noqa: E402
from proxy._aes import Cipher, algorithms, modes  # noqa: E402
from proxy.stats import stats  # noqa: E402

TAG = bytes.fromhex("dddddddd")  # padded intermediate (the dd mode)


def client_init(secret: bytes) -> bytes:
    """Same construction as upstream tests/test_h2_transport.py client_init."""
    raw = bytearray(os.urandom(64))
    raw[0] = 0x42
    raw[56:62] = TAG + struct.pack("<h", 2)
    keys = bytes(raw[8:56])
    up = Cipher(algorithms.AES(hashlib.sha256(keys[:32] + secret).digest()), modes.CTR(keys[32:])).encryptor()
    encrypted = up.update(bytes(raw))
    return bytes(raw[:56]) + encrypted[56:]


def legacy_connected(snap) -> bool:
    """The rule this replaces: active > 0 and cumulative relays > 0."""
    relayed = snap["connections_ws"] + snap["connections_h2"] + snap["connections_tcp_fallback"] + snap["connections_cfproxy"]
    return snap["connections_active"] > 0 and relayed > 0


def connected(snap) -> bool:
    return snap["relays_active"] > 0


async def until(cond, timeout=5.0):
    deadline = asyncio.get_running_loop().time() + timeout
    while not cond():
        if asyncio.get_running_loop().time() > deadline:
            raise AssertionError("condition not reached")
        await asyncio.sleep(0.02)


@pytest.fixture
def engine():
    """Upstream handler with Telegram replaced by a stub that holds each
    relayed connection until released."""
    release = {}

    async def fake_bridge(reader, writer, ws, label, ctx, **kw):
        ev = asyncio.Event()
        release[label] = ev
        await ev.wait()

    for k in vars(stats):
        setattr(stats, k, 0)
    with patch.object(tgp, "bridge_ws_reencrypt", fake_bridge), \
            patch.object(tgp.ws_pool, "get", AsyncMock(return_value=SimpleNamespace(send=AsyncMock()))):
        h._TRACKED.clear()
        h.RELAYS.active = h.RELAYS.total = 0
        h.install_relay_tracking()
        yield release
    h._TRACKED.clear()


def test_wrong_secret_never_connected_and_past_success_does_not_count(engine):
    release = engine
    secret = os.urandom(16)

    async def scenario():
        server = await asyncio.start_server(lambda r, w: tgp._handle_client(r, w, secret), "127.0.0.1", 0)
        port = server.sockets[0].getsockname()[1]
        try:
            # 1. A wrong secret alone: never connected.
            r, w = await asyncio.open_connection("127.0.0.1", port)
            w.write(client_init(os.urandom(16)))
            await w.drain()
            await until(lambda: stats.connections_bad == 1)
            snap = h.stats_snapshot()
            assert snap["connections_active"] == 1, "upstream counts it as active"
            assert not connected(snap)
            w.close()
            await until(lambda: stats.connections_active == 0)

            # 2. An authorised client relayed to Telegram: connected.
            r1, w1 = await asyncio.open_connection("127.0.0.1", port)
            w1.write(client_init(secret))
            await w1.drain()
            await until(lambda: h.RELAYS.active == 1)
            snap = h.stats_snapshot()
            assert connected(snap) and snap["connections_ws"] == 1

            # 3. It ends: all correct connections closed → not connected.
            for ev in list(release.values()):
                ev.set()
            await until(lambda: h.RELAYS.active == 0 and stats.connections_active == 0)
            snap = h.stats_snapshot()
            assert not connected(snap)
            assert snap["relays_total"] == 1 and snap["connections_ws"] == 1, "history kept"

            # 4. After that success, a client with a wrong secret: the old rule
            #    says "connected" (the bug), the new one does not.
            r2, w2 = await asyncio.open_connection("127.0.0.1", port)
            w2.write(client_init(os.urandom(16)))
            await w2.drain()
            await until(lambda: stats.connections_bad == 2)
            snap = h.stats_snapshot()
            assert legacy_connected(snap), "reproduces the false positive of the old rule"
            assert not connected(snap)
            w2.close()

            # 5. An authorised connection again, next to the bad one: connected.
            r3, w3 = await asyncio.open_connection("127.0.0.1", port)
            w3.write(client_init(secret))
            await w3.drain()
            await until(lambda: h.RELAYS.active == 1)
            assert connected(h.stats_snapshot())
            for ev in list(release.values()):
                ev.set()
            await until(lambda: h.RELAYS.active == 0)
            for w_ in (w1, w3):
                w_.close()
        finally:
            server.close()

    asyncio.run(scenario())


def test_every_bridging_path_is_tracked_exactly_once():
    import proxy.bridge as br
    h._TRACKED.clear()
    originals = {(m, n): getattr(m, n) for m, n in ((tgp, "bridge_ws_reencrypt"), (br, "bridge_ws_reencrypt"), (br, "_bridge_tcp_reencrypt"), (br, "bridge_h2"))}
    try:
        h.install_relay_tracking()
        h.install_relay_tracking()  # idempotent
        for (m, n), fn in originals.items():
            wrapped = getattr(m, n)
            assert wrapped.__wrapped__ is fn, f"{m.__name__}.{n} wrapped once"
    finally:
        for (m, n), fn in originals.items():
            setattr(m, n, fn)
        h._TRACKED.clear()
