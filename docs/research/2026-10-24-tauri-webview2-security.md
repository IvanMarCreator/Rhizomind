# Исследование: безопасность Tauri 2 и WebView2 (24.10.2026)

Источники — исходники tauri/wry/plugins-workspace (коммиты на 24.10.2026), v2.tauri.app, learn.microsoft.com. Метки: **факт** / **сторонний** / **оценка** / **неизвестно**.

## Tauri 2
- **факт** tauri 2.12.5, tauri-cli 2.12.4, wry 0.57.1, плагины dialog 2.8.3 и opener 2.7.2, tauri-driver 2.1.0. Минорные версии раз в 1–4 месяца, патчи еженедельно. С 13.09.2026 идёт tauri 3.0 alpha.
- **факт** Права: команды плагинов доступны интерфейсу только через permissions в capabilities; **свои команды по умолчанию доступны всем окнам**, пока не ограничены `AppManifest::commands`.
- **факт** CSP по умолчанию выключен; Tauri добавляет хэши своих скриптов. Isolation pattern (IPC через изолированный iframe, AES-GCM) рекомендован документацией; по умолчанию выключен. `freezePrototype` по умолчанию false.
- **факт** Сетевого клиента в ядре для компьютеров нет (reqwest — только мобильные). Updater, http, websocket — отдельные плагины, не ставим. Телеметрии в tauri/wry не найдено (оценка по поиску в исходниках).
- **факт** Плагин `opener` с правом `default` позволяет интерфейсу открыть любую http(s)/mailto/tel ссылку без подтверждения → интерфейсу его права не выдаём; открытие — своей командой Rust после подтверждения.
- **факт** Плагин `dialog` тянет `fs` и добавляет выбранные пути в его scope → права `fs:*` не выдаём.

## Как отдаётся интерфейс на Windows
- **факт** WebView2 не поддерживает свои схемы: wry перехватывает `http://<схема>.localhost` (`tauri.localhost`, `ipc.localhost`, `asset.localhost`). Запросы обслуживает процесс приложения, до сети они не доходят. На macOS/Linux — `tauri://localhost`, `ipc://localhost`.
- **оценка** CSP: `default-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self' asset: http://asset.localhost blob: data:`.

## Следы на диске
- **факт** Tauri по умолчанию кладёт папку данных WebView2 в `%LOCALAPPDATA%\<identifier>\`. Абсолютный путь (временная папка) — только из Rust: `WebviewWindowBuilder::data_directory`.
- **факт** `incognito(true)` = InPrivate. **оценка** Сама папка данных (Local State, кэш шейдеров, Crashpad) создаётся и так → нужны оба шага.
- **факт** Папку нельзя удалить, пока жив процесс браузера; удалять после завершения `BrowserProcessId` и дочерних. Очистка — забота приложения.
- **факт** При падении WebView2 пишет дамп и **отправляет его в Microsoft**, если `IsCustomCrashReportingEnabled = false` (по умолчанию). wry это свойство не выставляет, Tauri не открывает.

## Сеть WebView2
- **факт** wry по умолчанию передаёт `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`; свои `additional_browser_args` **заменяют** этот набор. `IsReputationCheckingRequired` (SmartScreen) выставляется через `with_webview`.
- **факт** Прочие службы из `edge://settings/privacy` в WebView2 выключены. **Обязательные диагностические данные WebView2 отправляются всегда**; необязательные — по настройке Windows. Отключить на уровне приложения нельзя.
- **факт** Experimentation and Configuration Service: WebView2 скачивает конфигурацию (на неуправляемом ПК — и эксперименты) с серверов Microsoft. Полностью отключает только политика `ExperimentationAndConfigurationServiceControl = 0`.
- **факт** Автозаполнение в Tauri по умолчанию включено — выключать в конфигурации окна.
- **факт** Из сетевых флагов WebView2 документирует только `--no-proxy-server` и прокси-флаги; `--host-resolver-rules`, `--disable-background-networking`, `--disable-component-update` — флаги Chromium, действие подтверждать аудитом.
- **факт** Переменные `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`, `WEBVIEW2_USER_DATA_FOLDER`, `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` и ключи реестра `...\Policies\Microsoft\Edge\WebView2\...` могут переопределить настройки извне (риск — и инструмент для ИТ).
- **неизвестно** Проверка орфографии (вероятно, средствами Windows без загрузки словарей). CodeMirror ставит `spellcheck="false"`.

## Evergreen или фиксированная версия

| Режим | Прибавка к установщику | Сеть при установке |
|---|---|---|
| downloadBootstrapper (по умолчанию) | 0 | да |
| embedBootstrapper | ~1,8 МБ | да |
| offlineInstaller | ~127 МБ | нет |
| fixedRuntime | ~180 МБ | нет |
| skip | 0 | нет; без установленного WebView2 не запустится |

- **факт** Evergreen есть в Windows 11 и почти на всех Windows 10; обновляет Microsoft через Edge Update.
- **факт** Fixed: свой `msedgewebview2.exe` в папке приложения; обновляем сами; на Windows 10 нужны права через `icacls`; с сетевого диска не запускается.
- **оценка** Fixed позволяет поставить правило брандмауэра «запретить исходящие» на путь своего движка, не задевая Teams и Outlook.

## macOS и Linux
- **факт** `incognito`: macOS — `nonPersistentDataStore`, Linux — эфемерный контекст. **сторонний** В WKWebView проверка мошеннических сайтов включена по умолчанию. **неизвестно** Записи в `~/Library/Caches`, `~/.cache`.

## Тестирование и сборка
- **факт** tauri-driver: msedgedriver (Windows), WebKitWebDriver (Linux); macOS — нет. Моки IPC `@tauri-apps/api/mocks` для тестов интерфейса.
- **факт** NSIS по умолчанию — установка для текущего пользователя без прав администратора; MSI (WiX) — только на Windows. Бандлер при сборке скачивает NSIS, WiX и установщики WebView2.
- **оценка** Сборка в CI: холодная 10–15 мин, с кэшем 3–5. Портативный exe работает без установки при наличии Evergreen.

## Меры для инвариантов на Windows

| № | Мера | Оценка |
|---|---|---|
| 1 | Окно создаётся в Rust: `incognito(true)`, `data_directory(%TEMP%\rhizomind-<случайно>)` | устранимо |
| 2 | При выходе ждать завершения процесса браузера и удалить папку; при запуске убрать остатки | устранимо |
| 3 | `additional_browser_args`: набор wry + `--no-proxy-server`, плюс флаги Chromium с подтверждением аудитом | нужна проверка |
| 4 | SmartScreen выключен (`IsReputationCheckingRequired = false`), автозаполнение выключено, инструменты разработчика в выпуске выключены | устранимо |
| 5 | Запрет навигации, новых окон, загрузок; CSP без внешних хостов; isolation pattern; `freezePrototype` | устранимо |
| 6 | Права `opener` интерфейсу не выдаются; ссылка открывается своей командой Rust после подтверждения | устранимо |
| 7 | Дампы падений не отправляются в Microsoft: патч wry с `IsCustomCrashReportingEnabled` | устранимо патчем |
| 8 | Обязательная телеметрия и сервис конфигурации WebView2 — компонент Windows | **приложением неустранимо**: политики ИТ, брандмауэр (при Fixed) |
| 9 | Переопределение через `WEBVIEW2_*` в окружении — удалять из окружения процесса при запуске | частично |
| 10 | Орфография, записи вне папки данных | нужна проверка (Process Monitor, сетевой аудит) |

## Источники
https://github.com/tauri-apps/tauri; https://github.com/tauri-apps/wry; https://github.com/tauri-apps/plugins-workspace; https://v2.tauri.app/security/csp/; https://v2.tauri.app/security/capabilities/; https://v2.tauri.app/concept/inter-process-communication/isolation/; https://v2.tauri.app/distribute/windows-installer/; https://v2.tauri.app/develop/tests/webdriver/; https://v2.tauri.app/develop/tests/mocking/; https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/user-data-folder; https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/webview-features-flags; https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution; https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/data-privacy; https://learn.microsoft.com/en-us/deployedge/microsoft-edge-webview-policies
