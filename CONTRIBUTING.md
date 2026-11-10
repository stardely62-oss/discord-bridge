# Участие в разработке

1. Прочитайте [ARCHITECTURE.md](ARCHITECTURE.md) и [docs/maintainers/building.md](docs/maintainers/building.md).
2. Создайте ветку от `main`, небольшие осмысленные коммиты.
3. Перед PR:
   ```bash
   cargo fmt --all
   cargo clippy -p bridge-core -p bridge-tools --all-targets -- -D warnings
   cargo test -p bridge-core -p discord-bridge-helper -p bridge-tools
   pnpm typecheck && pnpm lint && pnpm test && pnpm build
   ```
   Windows-части (служба, приложение) проверяются в CI на `windows-latest`.
4. Изменения в парсере BAT или таблице параметров сопровождайте тестами;
   тест `flowseal_fixtures.rs` должен оставаться зелёным.
5. Не добавляйте выполнение внешних команд, произвольные пути к файлам и новые
   привилегированные операции без обсуждения в issue.
6. Не коммитьте секреты, пользовательские конфигурации и бинарники Flowseal
   (они скачиваются и проверяются при сборке).

Язык интерфейса по умолчанию — русский; новые строки добавляйте через `tr("ru", "en")`.
