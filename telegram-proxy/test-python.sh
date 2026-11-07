#!/usr/bin/env bash
# Python tests of the Telegram WS Proxy engine (Linux CI, or locally):
# the upstream engine tests at the pinned commit (GUI/tray/updater tests
# excluded: those parts are not shipped) and the headless runner tests.
#   telegram-proxy/test-python.sh [workdir]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
work="${1:-${RUNNER_TEMP:-/tmp}/tgproxy-test}"
repo=$(python3 -c "import json;print(json.load(open('$here/upstream.lock.json'))['repository'])")
commit=$(python3 -c "import json;print(json.load(open('$here/upstream.lock.json'))['commit'])")
rm -rf "$work" && mkdir -p "$work"
git init -q "$work/src"
git -C "$work/src" fetch -q --depth 1 "$repo" "$commit"
git -C "$work/src" -c advice.detachedHead=false checkout -q FETCH_HEAD
test "$(git -C "$work/src" rev-parse HEAD)" = "$commit"
python3 -m venv "$work/venv"
"$work/venv/bin/pip" install --quiet --disable-pip-version-check --no-input --no-deps --only-binary=:all: --require-hashes \
  -r "$here/requirements-runtime.txt" -r "$here/requirements-test.txt"
cd "$work/src"
"$work/venv/bin/python" -m pytest -q -p no:cacheprovider tests \
  --ignore=tests/test_tray_common.py --ignore=tests/test_i18n.py \
  --ignore=tests/test_windows_update.py --ignore=tests/test_linux_update.py \
  --ignore=tests/test_update_check.py --ignore=tests/test_h2_settings.py
PYTHONPATH="$work/src" "$work/venv/bin/python" -m pytest -q -p no:cacheprovider "$here/tests"
