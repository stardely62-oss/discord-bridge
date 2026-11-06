# Логотип Discord Bridge

Утверждённый знак — разрезанный «пузырь» чата: верхняя половина светлая, нижняя
фиолетовая и смещена (связь между двумя сторонами). Файлы ниже взяты из
утверждённого пакета `bridge-logo.zip` без изменений; перерисовывать их нельзя.

| Файл | Назначение |
|---|---|
| `app-icon.svg` | иконка приложения (знак на тёмной плитке), основной вектор |
| `mark-on-dark.svg` | знак без плитки для тёмного фона (боковая панель, тёмная тема) |
| `mark-on-light.svg` | знак без плитки для светлого фона (светлая тема) |
| `png/app-<размер>.png` | 16, 32, 48, 64, 128, 256, 512, 1024 — из пакета, малые размеры уже адаптированы |
| `png/icon.ico` | Windows ICO из пакета: 16, 24, 32, 48, 64, 128, 256 |
| `png/mark-on-*-1024.png` | знак без плитки, растр |
| `tray-on.svg`, `tray-off.svg` | трей: иконка приложения без изменений + индикатор (зелёная точка — обход включён, серое кольцо — выключен) |
| `png/tray-on.png`, `png/tray-off.png` | трей, 64 px (рендер из SVG) |

Куда копируется (делается вручную, при обновлении пакета повторить):

```sh
pnpm tauri icon assets/brand/png/app-1024.png -o src-tauri/icons   # Store/Square-логотипы и icns
rm -rf src-tauri/icons/android src-tauri/icons/ios
cp assets/brand/png/icon.ico     src-tauri/icons/icon.ico          # ICO из пакета, не сгенерированный
cp assets/brand/png/app-32.png   src-tauri/icons/32x32.png
cp assets/brand/png/app-64.png   src-tauri/icons/64x64.png
cp assets/brand/png/app-128.png  src-tauri/icons/128x128.png
cp assets/brand/png/app-256.png  src-tauri/icons/128x128@2x.png
cp assets/brand/png/app-512.png  src-tauri/icons/icon.png
cp assets/brand/png/app-1024.png src-tauri/icons/app-icon.png
cp assets/brand/png/app-256.png  public/app-icon.png
cp assets/brand/mark-on-dark.svg  public/brand-mark-dark.svg
cp assets/brand/mark-on-light.svg public/brand-mark-light.svg
node scripts/icons/render.mjs /tmp/tray && cp /tmp/tray/tray-on.png /tmp/tray/tray-off.png src-tauri/icons/   # только трей
```
