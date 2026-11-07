# -*- mode: python ; coding: utf-8 -*-
# PyInstaller spec of the headless tg-ws-proxy engine for Discord Bridge.
# Built by build.ps1 with TGPROXY_UPSTREAM pointing at the pinned upstream
# checkout. Only the network engine (package `proxy`) is bundled: no tray,
# no GUI, no updater, no clipboard.
import os
from PyInstaller.utils.hooks import collect_data_files

upstream = os.environ["TGPROXY_UPSTREAM"]
here = os.path.dirname(os.path.abspath(SPEC))

a = Analysis(
    [os.path.join(here, "tgproxy_headless.py")],
    pathex=[upstream],
    binaries=[],
    datas=collect_data_files("certifi"),
    hiddenimports=[
        "httpx",
        "h2",
        "cryptography.hazmat.primitives.ciphers",
        "cryptography.hazmat.primitives.ciphers.algorithms",
        "cryptography.hazmat.primitives.ciphers.modes",
        "cryptography.hazmat.backends.openssl",
    ],
    excludes=[
        "tkinter", "customtkinter", "pystray", "PIL", "pyperclip", "psutil",
        "windows", "ui", "utils", "unittest", "pydoc", "test",
    ],
    noarchive=False,
)
pyz = PYZ(a.pure)
exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name="tg-ws-proxy-headless",
    debug=False,
    strip=False,
    upx=False,
    # Console subsystem for real stdin/stdout pipes; Discord Bridge starts it
    # with CREATE_NO_WINDOW, so no window appears.
    console=True,
    disable_windowed_traceback=True,
)
