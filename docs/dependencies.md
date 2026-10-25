# Зависимости Rhizomind

Каждая прямая зависимость — с точной версией, лицензией, назначением и ответом на вопрос «есть ли сетевой код». Новая зависимость добавляется сюда в той же задаче (план 0.1, раздел 1.2). Полный перечень с транзитивными зависимостями — SBOM (`npm run sbom`) и `cargo deny --manifest-path src-tauri/Cargo.toml list` (для генератора тестового хранилища — `cargo deny --manifest-path tools/gen-vault/Cargo.toml --config src-tauri/deny.toml list`; в проверке генератора отключены предупреждения о неиспользованных разрешениях лицензий и исключениях RustSec — они относятся к приложению).

Автоматические проверки при каждом `npm run verify`:
- лицензии крейтов, запрещённые крейты и функции — `cargo deny check` ([src-tauri/deny.toml](../src-tauri/deny.toml));
- сетевые крейты по каждой десктопной цели — [tools/check-cargo-tree/](../tools/check-cargo-tree/);
- лицензии production-зависимостей npm с разбором SPDX — [tools/check-licenses/](../tools/check-licenses/);
- сетевой код в исходниках и собранном интерфейсе — [tools/check-no-network/](../tools/check-no-network/).

## Ядро (Rust, `src-tauri/Cargo.toml`) — попадает в программу

| Крейт | Версия | Лицензия | Зачем | Сетевой код |
|---|---|---|---|---|
| `tauri` | 2.12.5 | MIT OR Apache-2.0 | Платформа: окно, WebView, команды ядра, разрешения (ADR 0008). Функции по умолчанию: `wry`, `compression`, `common-controls-v6`, `dynamic-acl`, `x11`, `dbus`; плюс `isolation` (isolation pattern, ADR 0009) | Нет для десктопа: `reqwest` у Tauri только для мобильных целей. `cargo tree` по Windows, macOS, Linux — пусто; функции `devtools`, `native-tls*`, `rustls-tls`, `protocol-asset` запрещены в `deny.toml` |
| `webview2-com` (только Windows) | 0.39.1 | MIT | Настройки WebView2, которых нет в Tauri: SmartScreen (`IsReputationCheckingRequired`), `BrowserProcessId` (задача 0.3). Та же версия уже приходит через wry — новых крейтов нет | Нет: привязки к COM-интерфейсам WebView2 |
| `windows` (только Windows) | 0.62.2 | MIT OR Apache-2.0 | `SetErrorMode`/`WerSetFlags` (отчёты WER), ожидание процессов движка (`OpenProcess`, снимок процессов). Та же версия уже приходит через wry и tao; функции перечислены в `Cargo.toml` | Нет: привязки к Win32 API, сетевых функций (`Win32_Networking_*`) не включено |
| `wry` — своя копия `vendor/wry/` | 0.57.1 | MIT OR Apache-2.0 | WebView (приходит через Tauri) с одной правкой `IsCustomCrashReportingEnabled = true`, подключена через `[patch.crates-io]` в корневом `Cargo.toml` (ADR 0007, `vendor/wry/RHIZOMIND-PATCH.md`) | Нет (как у wry 0.57.1 из crates.io; отличие — одна строка) |

Функция `isolation` приносит крейты шифрования RustCrypto: `aes-gcm` 0.10.3, `aes` 0.8.4, `aead` 0.5.2, `cipher` 0.4.4, `ctr` 0.9.2, `ghash` 0.5.1, `polyval` 0.6.2, `universal-hash` 0.5.1, `inout` 0.1.4, `opaque-debug` 0.3.1 (MIT OR Apache-2.0), `subtle` 2.6.1 (BSD-3-Clause), `getrandom` 0.2.17, `rand_core` 0.6.4 (MIT OR Apache-2.0) — без сетевого кода, проверены `cargo deny`.

Транзитивно приходят `tao` 0.37.2 (окна), `webview2-com` 0.39.1 (Windows), `webkit2gtk` 2.0.2 и `soup3` 0.5.0 (Linux — привязки к системным WebKitGTK и libsoup, через которые WebKitGTK обслуживает свои схемы адресов), `tokio` 1.54.1 без функции `net`. Лицензии всего дерева: MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-3-Clause, MPL-2.0, Unicode-3.0, Unlicense, Zlib, 0BSD, MIT-0, CC0-1.0 — все совместимы с GPL-3.0. Набор крейтов по каждой цели зафиксирован в `tools/check-cargo-tree/crates.json`: новый крейт валит `npm run verify`, пока его не проверят и не обновят список.

## Ядро — только сборка и тесты (в программу не попадают)

| Крейт | Версия | Лицензия | Зачем | Сетевой код |
|---|---|---|---|---|
| `tauri-build` | 2.7.4 | MIT OR Apache-2.0 | `build.rs`: встраивание конфигурации, `AppManifest::commands` | Нет |
| `tauri` с функцией `test` | 2.12.5 | MIT OR Apache-2.0 | Подменный runtime для тестов ядра без окна | Нет |
| `serde_json` | 1.0.154 | MIT OR Apache-2.0 | Тест конфигурации безопасности читает `tauri.conf.json` и файл разрешений | Нет |

## Генератор тестового хранилища (`tools/gen-vault/Cargo.toml`) — в программу не попадает

Отдельный крейт рабочего пространства, не зависимость `src-tauri`; `cargo deny` проверяет его с той же конфигурацией (`src-tauri/deny.toml`).

| Крейт | Версия | Лицензия | Зачем | Сетевой код |
|---|---|---|---|---|
| `serde` (функция `derive`) | 1.0.229 | MIT OR Apache-2.0 | Типы манифеста | Нет |
| `serde_json` | 1.0.154 | MIT OR Apache-2.0 | Манифест в JSON | Нет |
| `unicode-normalization` | 0.1.26 | MIT OR Apache-2.0 | Ключ имени (NFC) для дубликатов и ссылок; тот же крейт выбран для ядра (исследование крейтов) | Нет |

Транзитивно (`cargo tree -p gen-vault -e normal`): `serde_core`, `serde_derive`, `proc-macro2`, `quote`, `syn`, `unicode-ident` (макросы сборки), `itoa`, `memchr`, `zmij`, `tinyvec` — версии, которые уже были в `Cargo.lock`.

## Интерфейс (npm) — попадает в программу

| Пакет | Версия | Лицензия | Зачем | Сетевой код |
|---|---|---|---|---|
| `svelte` | 5.57.2 | MIT | Фреймворк интерфейса (ADR 0002) | Нет; собранный интерфейс проверяется `check-no-network` |
| `@tauri-apps/api` | 2.12.4 | Apache-2.0 OR MIT | `invoke` для команд ядра (только через `src/api/`) и подмена ядра в тестах | Нет: вызовы идут по IPC Tauri внутри процесса |

## Инструменты разработки (npm, `devDependencies`) — в программу не попадают

| Пакет | Версия | Лицензия | Зачем | Сетевой код |
|---|---|---|---|---|
| `@tauri-apps/cli` | 2.12.4 | Apache-2.0 OR MIT | `tauri dev`, `tauri build`, установщики | Да, только на машине сборки: бандлер скачивает WiX и NSIS при сборке MSI и NSIS |
| `vite` | 8.3.5 | MIT | Сборка интерфейса; сервер разработки на `localhost:5173` | Сервер разработки на localhost, только в `npm run dev` |
| `@sveltejs/vite-plugin-svelte` | 7.3.1 | MIT | Svelte в Vite | Нет |
| `typescript` | 6.0.3 | Apache-2.0 | Проверка типов (strict) | Нет |
| `svelte-check` | 4.7.7 | MIT | Проверка типов в `.svelte` | Нет |
| `vitest` | 5.0.5 | MIT | Модульные и сценарные тесты | Нет |
| `eslint` | 10.13.0 | MIT | Линтер | Нет |
| `@eslint/js` | 10.0.1 | MIT | Правила ESLint | Нет |
| `typescript-eslint` | 8.72.0 | MIT | ESLint для TypeScript | Нет |
| `@typescript-eslint/typescript-estree` | 8.72.0 | MIT | Синтаксическое дерево для `check-no-network` | Нет |
| `eslint-plugin-svelte` | 3.23.1 | MIT | ESLint для Svelte | Нет |
| `globals` | 17.13.0 | MIT | Списки глобальных имён для ESLint | Нет |
| `@types/node` | 22.20.5 | MIT | Типы Node для `tools/` и тестов | Нет |
| `webdriverio` | 10.1.0 | MIT | Клиент WebDriver для сценарных тестов | Да: HTTP-клиент к tauri-driver на `127.0.0.1:4444` во время теста. В его дереве есть пакеты `edgedriver` и `geckodriver`, которые в `postinstall` скачивают драйверы браузеров; скрипты установки выключены (`.npmrc`: `ignore-scripts=true`), `npm ci` ничего не скачивает |

## Инструменты, которые ставятся отдельно

| Инструмент | Версия | Лицензия | Зачем |
|---|---|---|---|
| Rust | 1.99.1 | MIT OR Apache-2.0 | Компилятор, закреплён в `rust-toolchain.toml` |
| `cargo-deny` | 0.20.2 | MIT OR Apache-2.0 | Лицензии, запреты, источники, уязвимости (база RustSec скачивается при проверке) |
| `tauri-driver` | 2.1.0 | Apache-2.0 OR MIT | WebDriver-сервер для сценарных тестов |
| `cargo-cyclonedx` | 0.5.9 | Apache-2.0 | SBOM ядра |
| msedgedriver | версия WebView2 | Microsoft | WebDriver для WebView2 на Windows (CI скачивает под установленную версию) |
| `webkit2gtk-driver` | из Ubuntu | LGPL-2.1 | WebDriver для WebKitGTK на Linux |

## Почему выбрано

- **Синтаксическое дерево для проверки сети — `@typescript-eslint/typescript-estree` + `svelte/compiler`.** Оба уже есть в проекте (линтер и сам Svelte), оба дают дерево ESTree, поэтому один обход проверяет `.ts`, собранный `.js` и выражения в разметке `.svelte`. TypeScript compiler API даёт своё дерево и не разбирает `.svelte`; `acorn` не понимает TypeScript без плагина.
- **Свой разбор SPDX вместо `spdx-expression-parse`.** Грамматика короткая (OR, AND, WITH, скобки), лишняя зависимость не нужна; тесты — в `tools/check-licenses/licenses.test.ts`.
