# Предложение авторам wry: настройка IsCustomCrashReportingEnabled

Для владельца. Ниже — готовый текст issue (и описание небольшого pull request) для репозитория [tauri-apps/wry](https://github.com/tauri-apps/wry), на английском. Отправляет владелец со своей учётной записи GitHub; агент в облаке не авторизован в `gh`. Если предложение примут и оно попадёт в Tauri, своя копия `vendor/wry/` и `[patch.crates-io]` в корневом `Cargo.toml` удаляются (ADR 0007, `vendor/wry/RHIZOMIND-PATCH.md`).

Что важно знать перед отправкой:
- В Tauri 2.12 уже есть обход без правки wry: `WebviewWindowBuilder::with_environment` (Windows) принимает готовое окружение WebView2, созданное приложением с любыми параметрами. Тогда приложение само задаёт папку данных, флаги и `IsCustomCrashReportingEnabled`. Мы пока держим правку wry по ADR 0007; этот обход стоит обсудить при следующем пересмотре ADR. Авторы wry могут предложить именно его — это нормальный ответ.
- Ссылка на документацию Microsoft: https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environmentoptions3

---

**Title:** Windows: allow setting `IsCustomCrashReportingEnabled` (opt out of sending WebView2 crash dumps to Microsoft)

**Body:**

### Problem

On Windows, WebView2 uploads crash dumps of its processes to Microsoft unless the host sets `ICoreWebView2EnvironmentOptions3::IsCustomCrashReportingEnabled = TRUE` when creating the environment. A dump of the renderer or browser process may contain page content. For privacy-sensitive, offline-only applications (we build a local notes app that has to pass a corporate security review) this is a data leak the app cannot currently prevent through wry/Tauri.

`webview2-com` already exposes the option (`CoreWebView2EnvironmentOptions::set_is_custom_crash_reporting_enabled`), but `InnerWebView::create_environment` in `src/webview2/mod.rs` never sets it, and there is no builder attribute for it.

### Proposal

Add a Windows-specific builder option, defaulting to the current behaviour:

```rust
// WebViewBuilderExtWindows
fn with_custom_crash_reporting(self, enabled: bool) -> Self;
```

and in `create_environment`:

```rust
options.set_is_custom_crash_reporting_enabled(pl_attrs.custom_crash_reporting);
```

Tauri could then expose it on `WebviewBuilder` / `WebviewWindowBuilder` (and possibly in `tauri.conf.json` window config).

### Evidence

We verified on GitHub Actions `windows-latest` (WebView2 Runtime 155.0.4283.62): with the option set to `TRUE`, the browser process is started with `--edge-webview-disable-crash-reporting=1`; without it the switch is absent. Crash dumps are still written locally to `<user data folder>\EBWebView\Crashpad\reports\` (as documented — the host may collect them via `ICoreWebView2Environment11::FailureReportFolderPath`).

### Workaround we use today

A vendored copy of wry 0.57.1 with a one-line change (`options.set_is_custom_crash_reporting_enabled(true);` in `create_environment`), applied via `[patch.crates-io]`. We'd be happy to send a PR with the builder option if the approach is acceptable.
