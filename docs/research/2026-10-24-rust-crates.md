# Исследование: крейты Rust для ядра 0.1–0.3 (24.10.2026)

Версии и лицензии — из API crates.io; поведение — по исходникам крейтов. Минимальный проект `tauri = "2"` (2.12.5) с плагинами opener и dialog и всеми кандидатами собран, `cargo tree` снят для Windows, macOS и Linux. Метки: **факт** / **оценка**.

## Рекомендации

| Задача | Крейт | Почему | Риски |
|---|---|---|---|
| Слежение за папкой | `notify` 9.0.0-rc.6 (CC0) + `notify-debouncer-full` 0.8.0-rc.2 | **факт:** в 8.2.0 переполнение буфера на Windows (16 КБ) молча теряется; в rc.5 исправлено — приходит событие Rescan. Один дескриптор на всё дерево. Debouncer склеивает пары переименования. | Кандидат в выпуск — фиксировать точно. **оценка:** массовые изменения почти всегда дают Rescan → полная сверка с диском. События `.rhizomind/` фильтровать самим. Запасной путь — свой наблюдатель на `windows` 0.62. |
| Корзина | `trash` 5.2.9 (MIT) | Windows `IFileOperation` с `FOF_ALLOWUNDO`; без сети. | `unwrap()` на COM — вызывать в отдельном потоке. **оценка:** на томе без корзины Windows может удалить безвозвратно — проверять заранее (инвариант 4). macOS — выбрать `NsFileManager`. |
| Атомарная запись | `tempfile` 3.27 (`NamedTempFile::new_in(".rhizomind/tmp")` → `sync_all` → `persist`) | **факт:** `persist` = `MoveFileExW(REPLACE_EXISTING)`. | **оценка:** замена меняет идентификатор NTFS и, возможно, дату создания; разрешение дублей идёт по дате создания → `ReplaceFileW` или хранить «дату появления» в индексе. |
| Версия файла | `blake3` 1.8 + размер и дата изменения | Хэш для `note.write(текст, ожидаемая версия)`. | — |
| Идентификатор файла | `file-id` 0.2.3 | **факт:** 128-битный FileId; в std аналог нестабилен. | — |
| Контур хранилища | `dunce` 1.0.5 + своя проверка; `junction` 2.1 — в тестах | **факт:** `is_symlink()` на Windows истинен и для точек соединения NTFS. | `cap-std` 4.0 — возможный второй слой позже (на Windows остаётся гонка «проверил — открыл»). |
| Unicode | `unicode-normalization` 0.1.26 + `icu_casemap` 2.4 | Ключ имени: NFC → полное приведение регистра → NFC; один ключ для дублей и ссылок. | Наше правило строже NTFS (ß = SS) — безопасно. |
| Сортировка | `Intl.Collator('ru', {numeric: true})` в интерфейсе | ICU уже в WebView. | В ядре при необходимости — `icu_collator`. |
| Имена Windows, BOM, переносы | свои функции | Короткие, с пояснениями для интерфейса. Невалидный UTF-8 → только чтение. | — |
| Нечёткий поиск (Ctrl+O) | `nucleo-matcher` 0.3.1 (MPL-2.0) в ядре | Тот же ключ нормализации; в интерфейс — топ-50. | Последний выпуск 02.2024; запасной `frizbee` 0.13 (MIT). |
| Индекс 0.2–0.3 | `rusqlite` 0.41, `bundled` (SQLite 3.54, FTS5 включён) | Одна база `.rhizomind/index.db` для ссылок и поиска. | **факт:** временные файлы SQLite по умолчанию — во временной папке ОС → собирать с `-DSQLITE_TEMP_STORE=3 -DSQLITE_OMIT_LOAD_EXTENSION` и `PRAGMA temp_store=MEMORY`. Русский стемминг — `frostem` (BSD-3). `tantivy` — запасной вариант. |
| Markdown (0.2) | `pulldown-cmark` 0.13.5, без функций по умолчанию | **факт:** `ENABLE_WIKILINKS`, смещения в тексте, правильный пропуск кода. | Общий набор образцов с разбором CodeMirror (Lezer). |
| Настройки | `serde_json`; путь — `dirs` 7.0 (`config_dir()/Rhizomind/vaults.json`) | **факт:** Tauri `app_config_dir()` называет папку по identifier. | — |
| Внешние ссылки | `open` 5.4.5 (MIT) из Rust после подтверждения | Не выставлять JS-команды opener в интерфейс. | — |
| Тесты | `tempfile`, `proptest` 1.12, `insta` 1.49 | Свойства путей и ссылок; снимки. | Символическая ссылка требует прав (режим разработчика); на windows-latest права есть. |

## Контур хранилища: порядок проверки

1. Интерфейс передаёт относительный путь из компонентов; каждый проверяется: нет `..`, `.`, `:`, разделителей, зарезервированных имён, точки или пробела в конце; абсолютные пути и `\\?\` отклоняются.
2. Корень канонизируется один раз при открытии (`dunce::canonicalize`).
3. Каждый промежуточный компонент — `symlink_metadata`; символическая ссылка или точка соединения → отказ.
4. Итоговый путь — `starts_with(root)`.

## Сеть в зависимостях (факт)

`cargo tree -e normal --target <T> | grep -E "hyper|reqwest|ureq|h2 |rustls|native-tls|openssl|socket2|tungstenite"`:
- Windows x64, macOS arm64 — пусто;
- Linux — только `mio` (для inotify) без функции `net`.

`tokio` на десктопе — без `net`. `reqwest` в `cargo tree --target all` — только мобильные цели Tauri, поэтому проверки ограничиваются десктопными целями. `tauri-plugin-dialog` тянет `tauri-plugin-fs` — в capabilities не выдавать `fs:*`.

## Проверки цепочки поставок

- `cargo-deny` 0.20 — лицензии, запрет сетевых крейтов и функций, источники; конфигурация ниже.
- `cargo-about` 0.9 — список лицензий для ИБ; `cargo-cyclonedx` 0.5 — SBOM в выпуск.
- В CI дополнительно — `cargo tree` по каждой цели с grep.

```toml
[graph]
targets = ["x86_64-pc-windows-msvc","aarch64-pc-windows-msvc","x86_64-apple-darwin","aarch64-apple-darwin","x86_64-unknown-linux-gnu"]

[advisories]
version = 2
yanked = "deny"

[licenses]
version = 2
allow = ["MIT","MIT-0","Apache-2.0","Apache-2.0 WITH LLVM-exception","BSD-2-Clause","BSD-3-Clause","ISC","Zlib","0BSD","CC0-1.0","Unlicense","BSL-1.0","MPL-2.0","Unicode-3.0","GPL-3.0-only","GPL-3.0-or-later"]

[bans]
wildcards = "deny"
deny = [
  { crate = "reqwest" }, { crate = "hyper" }, { crate = "hyper-util" }, { crate = "h2" }, { crate = "h3" },
  { crate = "ureq" }, { crate = "isahc" }, { crate = "curl" }, { crate = "curl-sys" }, { crate = "attohttpc" },
  { crate = "surf" }, { crate = "quinn" }, { crate = "socket2" }, { crate = "rustls" }, { crate = "tokio-rustls" },
  { crate = "native-tls" }, { crate = "openssl" }, { crate = "openssl-sys" }, { crate = "tungstenite" },
  { crate = "tokio-tungstenite" }, { crate = "tauri-plugin-http" }, { crate = "tauri-plugin-updater" },
  { crate = "tauri-plugin-websocket" }, { crate = "tauri-plugin-upload" }, { crate = "tauri-plugin-localhost" },
  { crate = "tauri-plugin-shell" },
]
[[bans.features]]
crate = "tokio"
deny = ["net"]
[[bans.features]]
crate = "mio"
deny = ["net"]
[[bans.features]]
crate = "tauri"
deny = ["devtools","native-tls","native-tls-vendored","rustls-tls"]

[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

## Заметка для облачной среды

cargo работает через прокси: `NO_PROXY=localhost`, `CARGO_HTTP_PROXY=$HTTPS_PROXY`, `CARGO_HTTP_CAINFO=/root/.ccr/ca-bundle.crt`.

## Источники

crates.io API по каждому крейту; https://github.com/notify-rs/notify/pull/964; исходники tempfile, trash, rusqlite (libsqlite3-sys/build.rs), std Rust 1.97 (`sys/fs/windows.rs`); https://github.com/tauri-apps/tauri (Cargo.toml: reqwest только для мобильных); https://embarkstudios.github.io/cargo-deny/; https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-replacefilew; https://learn.microsoft.com/windows/win32/fileio/naming-a-file; https://docs.rs/tauri/latest/tauri/path/struct.PathResolver.html
