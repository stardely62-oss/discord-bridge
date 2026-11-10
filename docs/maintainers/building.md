# Сборка

## Требования

- Windows 11 x64, Rust stable (MSVC), Node.js 20+, pnpm 9, WebView2 Runtime.
- Для ядра на Linux/macOS достаточно Rust: `cargo test -p bridge-core`.

## Шаги

```powershell
pnpm install
node scripts/fetch-flowseal.mjs        # скачивает официальный релиз Flowseal, сверяет SHA-256, распаковывает
cargo build --release -p discord-bridge-helper
New-Item -ItemType Directory -Force src-tauri/binaries
Copy-Item target/release/discord-bridge-helper.exe src-tauri/binaries/discord-bridge-helper-x86_64-pc-windows-msvc.exe
pnpm tauri build                       # NSIS-установщик в target/release/bundle/nsis/
```

`pnpm tauri build` перед сборкой приложения запускает `node scripts/third-party-licenses.mjs`.
Скрипт собирает лицензии Rust-крейтов и npm-пакетов, которые входят в приложение, в
`resources/third-party/` (в установке это `licenses/third-party/`). Если у пакета нет
лицензии или она не из списка разрешённых, сборка останавливается. Для `cargo clippy`
и тестов `src-tauri` эта папка тоже должна существовать: запустите скрипт после
`pnpm build`.

При обновлении Flowseal обновите `licenses/third-party-sources.json` и таблицу в
`THIRD_PARTY_NOTICES.md` (хеши, версии, ссылки на исходники). Иначе проверка
установщика в CI не пройдёт.

Для разработки UI: `pnpm tauri dev`. Обход DPI в dev-режиме работать не будет: служба
принимает только `discord-bridge.exe` из каталога установки.

## Обновление закреплённой версии Flowseal

1. Проверьте новый релиз Flowseal (изменения, лицензию, состав).
2. Обновите `resources/flowseal.lock.json` (версия, URL, SHA-256, `archiveRoot`).
3. `node scripts/fetch-flowseal.mjs` — инструмент откажется, если какая-либо
   стратегия не импортируется без ошибок.
4. Обновите фикстуры в `crates/bridge-core/tests/fixtures/` и прогоните тесты.

## Подписанный манифест для обновления Flowseal у пользователей

Workflow **Flowseal manifest** (ручной запуск): параметры `version` и проверенный
`sha256` архива. Он скачивает официальный архив, проверяет хэш, проверяет
импорт всех стратегий, подписывает манифест ключом из секрета
`FLOWSEAL_MANIFEST_SIGNING_KEY` и публикует релиз `flowseal-<version>` с файлом
`flowseal-<version>.signed.json`.

## Релиз приложения

Тег `vX.Y.Z[-pre]` запускает `release.yml`: сборка на `windows-latest`, NSIS-установщик,
`SHA256SUMS.txt`, описание релиза с версиями Flowseal/winws/WinDivert.

## Предпросмотр интерфейса без Windows

`scripts/preview/` — Vite-конфигурация с подставным бэкендом Tauri (реальный каталог
стратегий из `bridge-tools catalog`, параметры из `bridge-tools specs`):

```bash
cargo run -p bridge-tools -- catalog resources/flowseal/1.10.4 > scripts/preview/catalog.json
cargo run -p bridge-tools -- specs > scripts/preview/specs.json
pnpm vite --config scripts/preview/vite.config.ts   # http://localhost:1430/?s=running|stopped|onboarding
```

### Тесты интерфейса

`e2e/` — сценарии Playwright поверх того же предпросмотра: настоящий React-интерфейс,
подставной бэкенд. Проверяется, что разделы открываются без ошибок, какие команды
интерфейс отправляет бэкенду (`window.__bridgeCalls` в `mock-core.ts`) и что статусы
не показывают неподтверждённого. Службу, движок и Windows эти тесты не затрагивают.

```bash
pnpm exec playwright install chromium   # один раз
pnpm e2e                                # сам поднимает сервер предпросмотра
```

Нужны `catalog.json` и `specs.json` из команд выше. В CI это задание «Interface».

## Telegram WS Proxy engine

Before `pnpm tauri build` on Windows, build the headless engine (Python 3.11.9 must be on PATH):

```powershell
./telegram-proxy/build.ps1
```

It writes `resources/tg-ws-proxy/`, which is not tracked in git; the build compiles the engine's SHA-256 into the app.
Without this step the Telegram module is reported as not installed, and the Tauri bundler stops because the resource folder is missing.
Python tests on Linux: `telegram-proxy/test-python.sh`.
