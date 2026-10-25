# Своя копия wry 0.57.1 (Rhizomind)

Это исходники крейта [`wry` 0.57.1](https://crates.io/crates/wry/0.57.1) из архива crates.io (SHA-256 `759c50ddad0ef9913bb5496cb9d509a50433a3b78cb0238d61105526589cf0e9` — та же сумма, что была в `Cargo.lock`) с **одной правкой**. Подключается в корневом `Cargo.toml` (рабочее пространство; в `src-tauri/Cargo.toml` Cargo `[patch]` молча игнорирует):

```toml
[patch.crates-io]
wry = { path = "vendor/wry" }
```

Лицензия wry — MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE` рядом), совместима с GPL-3.0. Остальные файлы (в том числе `.cargo/`, `Cargo.lock`, `examples/`) оставлены как в архиве: они не участвуют в сборке Rhizomind, но позволяют сравнить копию с оригиналом целиком.

## Что изменено

Файл `src/webview2/mod.rs`, функция `InnerWebView::create_environment` (только Windows): перед `CreateCoreWebView2EnvironmentWithOptions` добавлена строка

```rust
options.set_is_custom_crash_reporting_enabled(true);
```

## Почему

По умолчанию WebView2 отправляет дампы падений своих процессов в Microsoft (`ICoreWebView2EnvironmentOptions3::IsCustomCrashReportingEnabled = FALSE`). Дамп процесса интерфейса может содержать текст открытых заметок. wry это свойство не выставляет, Tauri его не открывает (ADR 0007, инвариант 3 в `CLAUDE.md`).

Проверено в CI на Windows (WebView2 155): с правкой процесс браузера запускается с ключом `--edge-webview-disable-crash-reporting=1`, без неё — без ключа. Этот ключ проверяет интеграционный тест `src-tauri/tests/webview2_runtime.rs`.

Что правка **не** делает: WebView2 по-прежнему пишет дамп локально в `<папка данных>\EBWebView\Crashpad\reports\` (так он устроен; приложение может забрать дамп само, мы этого не делаем). Папка данных Rhizomind — временная и удаляется после выхода (`src-tauri/src/webview_traces/`), см. `docs/SECURITY.md`.

## Как проверить копию

```sh
curl -L https://static.crates.io/crates/wry/wry-0.57.1.crate -o wry.crate
sha256sum wry.crate      # 759c50ddad0ef9913bb5496cb9d509a50433a3b78cb0238d61105526589cf0e9
mkdir orig && tar -xzf wry.crate -C orig
diff -r orig/wry-0.57.1 vendor/wry   # отличаются только src/webview2/mod.rs и этот файл
```

Тест `src-tauri/tests/security_config.rs` (`wry_comes_from_own_patched_copy_of_the_same_version`) проверяет, что в `Cargo.lock` wry взят из этой копии (без `source`), версия 0.57.1 и правка на месте. Если Tauri перейдёт на другую версию wry, cargo молча перестанет применять `[patch]` — тест это поймает.

## Как перенести на новую версию wry

1. Узнать версию wry в новом `Cargo.lock` (`cargo tree -i wry`).
2. Заменить содержимое `vendor/wry/` распакованным архивом этой версии (первый коммит — без изменений, отдельно).
3. Повторить правку из раздела «Что изменено» (одна строка) и этот файл; обновить версию в тесте `security_config.rs`.
4. `npm run verify` и CI на Windows: интеграционный тест читает ключ `--edge-webview-disable-crash-reporting=1`.

Если авторы wry примут предложение (`docs/upstream/wry-crash-reporting.md`), копия удаляется вместе с `[patch.crates-io]`.
