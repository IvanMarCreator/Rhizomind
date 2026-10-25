# Модель безопасности Rhizomind (Secure-редакция)

Документ для службы информационной безопасности. Каждая мера ссылается на файл, где она реализована, чтобы её можно было проверить в коде. Документ обновляется вместе с каждым выпуском; выпуск не считается готовым, пока его меры не внесены сюда.

> **Состояние.** Платформа — Tauri 2: ядро на Rust, интерфейс в системном WebView2 ([ADR 0008](adr/0008-platform-tauri2.md)). Код выпуска 0.1 пишется по задачам плана. Пометка **«реализовано»** — мера есть в коде, рядом ссылка на файл и тест; пометка «выпуск 0.1, ADR …» — мера ещё в работе, указан файл, где она будет. Каркас приложения (задача 0.2) реализовал проверки цепочки поставок, CSP, `freezePrototype`, права окна и режим установщика. Прототип `prototype/` и ветка `archive/electron-skeleton` (Electron) — справочные, их меры к выпуску не относятся. Целевые требования — в [product/requirements.md](product/requirements.md#безопасность).

## Ключевые гарантии

1. **В коде приложения нет сети.** Ядро на Rust не содержит сетевых библиотек — это проверяется автоматически при каждой сборке (`cargo deny check`, `cargo tree`). Нет синхронизации, облака, телеметрии, автообновлений, загрузки внешних ресурсов, встроенного браузера и плагинов. Внешняя ссылка открывается в системном браузере только после подтверждения пользователя.
2. **Данные только в хранилище.** Заметки — обычные `.md`-файлы в папке, которую выбрал пользователь. Приложение читает и пишет только внутри неё. Служебные данные лежат в `.rhizomind/` внутри хранилища. Вне хранилища — один файл в профиле ОС со списком хранилищ (имена и пути), последним открытым хранилищем и размером окна.
3. **Браузерный движок ничего не сохраняет.** WebView2 работает в режиме InPrivate; его служебная папка — во временной папке ОС и удаляется после выхода. Дампы падений в Microsoft не отправляются.
4. **Изоляция интерфейса.** Интерфейс не имеет доступа к файловой системе и сети. Он может только вызывать явно разрешённый набор команд ядра над открытым хранилищем.
5. **Без потери данных.** Приложение не затирает изменения файлов, сделанные снаружи. Удаление — только в корзину ОС.

### Честно о WebView2
WebView2 — компонент Windows (движок Microsoft Edge), на котором работают, например, новые Outlook и Teams. Rhizomind использует **системную версию WebView2 (Evergreen)**, которую обновляют Microsoft или служба ИТ ([ADR 0007](adr/0007-webview2-traces-network.md)).
- Все сетевые функции WebView2, которые может выключить приложение, выключены (таблица «Сеть WebView2» ниже).
- **Обязательные диагностические данные WebView2** отправляются в Microsoft всегда; необязательные — по настройке диагностики Windows. Это поведение компонента Windows, приложение его отключить не может. Содержимое заметок в этих данных приложением не передаётся.
- **Experimentation and Configuration Service:** WebView2 получает конфигурацию с серверов Microsoft (на неуправляемом ПК — и эксперименты). Полностью отключается политикой Windows `ExperimentationAndConfigurationServiceControl = 0` (раздел реестра политик Microsoft Edge WebView2), которую выставляет служба ИТ.
- **Можно отказаться от этих обращений**, если поставлять с приложением встроенную фиксированную версию WebView2: тогда служба ИТ ставит на путь движка правило брандмауэра «запретить исходящие», не задевая Teams и Outlook. Недостатки: установщик больше примерно на 180 МБ; обновления движка выпускаем мы, каждое — новый выпуск и, вероятно, повторное согласование; на Windows 10 при установке нужны права на папку движка (`icacls`); с сетевого диска не запускается. Этот вариант не разрабатывается; к нему вернёмся, только если ИБ отклонит выпуск из-за обращений WebView2 к Microsoft.

## Меры защиты

### Сеть: код приложения

| Мера | Где |
|---|---|
| Запрет сетевых крейтов (`reqwest`, `hyper`, `h2`, `rustls`, `native-tls`, `openssl`, `socket2`, `tungstenite` и др.), сетевых плагинов Tauri (`http`, `updater`, `websocket`, `upload`, `localhost`, `shell`) и функций `tokio/net`, `mio/net`; неизвестные реестры и git-источники запрещены | **реализовано:** [src-tauri/deny.toml](../src-tauri/deny.toml), запуск в `npm run verify` и CI ([.github/workflows/ci.yml](../.github/workflows/ci.yml)), ADR 0009 |
| Независимая проверка `cargo tree` по каждой десктопной цели (Windows, macOS, Linux): сетевых крейтов нет | **реализовано:** [tools/check-cargo-tree/](../tools/check-cargo-tree/) (`cargo tree -e normal --target <цель>` для `x86_64`/`aarch64` Windows и macOS и `x86_64` Linux; кроме имён ловит `tokio`/`mio` с функцией `net`; ещё `cargo tree -e normal,build` — без крейтов только для тестов `gen-vault` и `proptest`), тесты `tree.test.ts`, ADR 0009 |
| Проверка интерфейса на сетевой код по синтаксическому дереву, в исходниках (`src/`) и в собранном интерфейсе (`dist/`): сетевые модули Node в любом виде импорта, сетевые плагины Tauri и `opener`/`shell`; `fetch`, `Request`, `XMLHttpRequest`, `WebSocket`, `EventSource`, `sendBeacon`, `RTCPeerConnection`, `WebTransport`, `Worker`/`SharedWorker`/`serviceWorker`; `window.open`, присваивание `location`, `location.assign/replace`; внешний и вычисляемый `import()`/`require()`; как подозрительное — обращение к глобальному объекту по вычисляемому имени (в том числе через псевдоним), `eval`/`Function`, элементы `<iframe>`, `<script>`, `<form>` и др. в разметке; файл, который не разобрался, — тоже находка | **реализовано:** [tools/check-no-network/](../tools/check-no-network/), тесты `scan.test.ts`, `paths.test.ts`, ADR 0009 |
| Политика контента (CSP) без внешних хостов: `connect-src ipc: http://ipc.localhost`, только свои скрипты и стили. Для `npm run dev` отдельная `devCsp` добавляет только сервер разработки `ws://localhost:5173` и встроенные стили Vite; в выпускную сборку она не попадает | **реализовано:** `app.security.csp` в [src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json), тест [src-tauri/tests/security_config.rs](../src-tauri/tests/security_config.rs), ADR 0009 |
| Запрещены переходы на другие адреса, новые окна и загрузки | `src-tauri/src/security/` — выпуск 0.1, ADR 0009 |
| Внешняя ссылка: только `http`, `https`, `mailto`; подтверждение в окне ядра с полным адресом; передача ОС после подтверждения | `src-tauri/src/external_links/` — выпуск 0.1, ADR 0009 |
| Автообновлений нет: плагин `updater` запрещён | **реализовано:** [src-tauri/deny.toml](../src-tauri/deny.toml) |
| Свои CSS-темы не могут загрузить внешние ресурсы (`@import`, `url()` наружу блокируются CSP) | выпуск 0.5 |

### Сеть WebView2

| Мера | Где |
|---|---|
| Флаги движка: набор wry по умолчанию (`--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`) + `--no-proxy-server` (нет поиска прокси WPAD) | `src-tauri/src/security/` — выпуск 0.1, ADR 0007 |
| Флаги против фоновой сети: `--host-resolver-rules="MAP * ~NOTFOUND"`, `--disable-background-networking`, `--disable-component-update`. Для WebView2 не документированы — действие каждого подтверждается сетевым аудитом перед выпуском; неподтверждённый флаг гарантией не считается | `src-tauri/src/security/` — выпуск 0.1, ADR 0007 |
| SmartScreen (проверка адресов на серверах Microsoft) выключен: `IsReputationCheckingRequired = false` | `src-tauri/src/security/` — выпуск 0.1, ADR 0007 |
| Отчёты о падениях не отправляются в Microsoft: своя правка wry выставляет `IsCustomCrashReportingEnabled = true`; своего сбора дампов нет | `vendor/wry/`, `[patch.crates-io]` в `src-tauri/Cargo.toml` — выпуск 0.1, ADR 0007 |
| Автозаполнение форм выключено | `src-tauri/src/security/` — выпуск 0.1, ADR 0007 |
| Проверка орфографии выключена в редакторе (`spellcheck="false"`) | `src/editor/` — выпуск 0.1 |
| Переменные окружения `WEBVIEW2_*` (могут подменить флаги, папку данных и движок) удаляются из окружения процесса при запуске | `src-tauri/src/security/` — выпуск 0.1, ADR 0007 |
| Обязательная диагностика и сервис конфигурации системной версии WebView2 — **приложением неустранимы**; закрываются настройками диагностики Windows и политикой `ExperimentationAndConfigurationServiceControl = 0` | политики службы ИТ, ADR 0007 |

### Хранение и следы вне хранилища

| Мера | Где |
|---|---|
| WebView2 в режиме InPrivate (`incognito`): кэш, cookies и хранилища страницы только в памяти | `src-tauri/src/security/` — выпуск 0.1, ADR 0007 |
| Папка данных WebView2 (`Local State`, кэш шейдеров и др.) — во временной папке ОС `rhizomind-<случайное>`, а не в `%LOCALAPPDATA%`; содержимого заметок в ней нет | `src-tauri/src/webview_traces/` — выпуск 0.1, ADR 0007 |
| Папка удаляется после завершения процесса браузера; остатки после сбоя — при следующем запуске | `src-tauri/src/webview_traces/` — выпуск 0.1, ADR 0007 |
| В профиле ОС один файл: `%APPDATA%\Rhizomind\vaults.json` (список хранилищ, последнее открытое, размер окна) | `src-tauri/src/registry/` — выпуск 0.1 |
| Служебные данные хранилища — только в `.rhizomind/` внутри него | выпуск 0.1 (настройки), 0.2 (индекс), 0.4 (раскладка) |
| Временные файлы SQLite только в памяти (`SQLITE_TEMP_STORE=3`, `PRAGMA temp_store=MEMORY`), загрузка расширений SQLite невозможна | выпуск 0.3, ADR 0004 |
| Журнал ошибок в `.rhizomind/logs`: без содержимого заметок, с ограничением размера | выпуск 0.5 |

### Файлы

| Мера | Где |
|---|---|
| Доступ только внутри открытого хранилища: путь от интерфейса — относительный, по компонентам; `..`, абсолютные пути и `\\?\` отклоняются | `src-tauri/src/vault/` — выпуск 0.1 |
| Зарезервированные имена Windows (`CON`, `NUL`, `COM1`… в том числе с расширением) отклоняются в каждом компоненте пути | `src-tauri/src/vault/`, `src-tauri/src/names/` — выпуск 0.1, ADR 0009 |
| Имя нового или переименованного файла и папки проверяется правилами Windows до обращения к диску: запрещённые и управляющие символы, `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`, `COM0`–`COM9`, `LPT0`–`LPT9`, `COM¹²³`, `LPT¹²³` с любым расширением, точка или пробел в конце имени и основы имени заметки, длина больше 255 единиц UTF-16. Команды `note_create`, `folder_create`, `entry_rename`, `vault_create`, `name_check` отклоняют такое имя с причиной (`invalid_name`) | **реализовано:** [src-tauri/src/names/mod.rs](../src-tauri/src/names/mod.rs) (`check`, `require_valid`), вызов — [src-tauri/src/commands/files.rs](../src-tauri/src/commands/files.rs); тесты `names::tests` и `create_and_rename_check_name_rules_before_modules` в [src-tauri/src/commands/acl_tests.rs](../src-tauri/src/commands/acl_tests.rs) (`CON.md`, `a:b.md` → `invalid_name` до модуля хранилища) |
| Совпадение имён (дубликаты в папке, ссылки, поиск) — по ключу имени NFC → полное приведение регистра → NFC, плюс `ı` → `i` (так NTFS сравнивает `ı.md` и `I.md`). Правило строже NTFS: имена, которые NTFS считает одним, ядро тоже считает одним (проверено перебором символов при ревью 1.1), а некоторые, которые NTFS различает, ядро считает одним — `ß` = `SS` и имена, различающиеся только формой Unicode (составная и разложенная «й», NFC/NFD), получают один ключ | **реализовано:** [src-tauri/src/names/mod.rs](../src-tauri/src/names/mod.rs) (`key`); свойства `proptest` |
| Цель вики-ссылки с `..`, `.`, пустым компонентом, `\`, `:` и другими запрещёнными символами не разрешается (неразрешённая ссылка, не ошибка); ссылка ищется только в списке заметок хранилища в памяти, без обращения к диску | **реализовано:** [src-tauri/src/links/mod.rs](../src-tauri/src/links/mod.rs) (`parse_target`, `resolve`); общие образцы [tests/fixtures/wikilinks/parse.json](../tests/fixtures/wikilinks/parse.json) |
| Имена с `#`, `[`, `]` или пробелом в начале допустимы (как в Windows), но `name_check` возвращает предупреждение `unlinkable`, а подсказки вики-ссылок показывают такую заметку с `linkable: false` и без вставляемого текста (С3.2) | **реализовано:** [src-tauri/src/names/mod.rs](../src-tauri/src/names/mod.rs) (`warning`, `is_linkable`), [src-tauri/src/commands/files.rs](../src-tauri/src/commands/files.rs) (`name_check`), [src-tauri/src/links/mod.rs](../src-tauri/src/links/mod.rs) (`suggest`) |
| Символические ссылки и точки соединения NTFS не используются для выхода за контур (проверка каждого компонента) | `src-tauri/src/vault/` — выпуск 0.1 |
| Атомарная запись через `.rhizomind/tmp/`; запись сверяется с версией файла на диске, внешние изменения не затираются | `src-tauri/src/vault/` — выпуск 0.1 |
| Ограничение размера редактируемого файла (по умолчанию 10 МБ, настройка хранилища) | `src-tauri/src/vault/` — выпуск 0.1 |
| Удаление в корзину ОС; на диске без корзины (флешка, сетевая папка) — безвозвратно, только после отдельного подтверждения пользователя | `src-tauri/src/vault/` — выпуск 0.1 |
| Слежение за папкой через уведомления ОС (`notify`, без сетевого кода); своя запись отличается от внешней по версии файла | `src-tauri/src/watcher/` — выпуск 0.1, ADR 0006 |
| Вложения (в том числе из буфера обмена) сохраняются только внутри хранилища | выпуск 0.3 |

### Изоляция интерфейса

| Мера | Где |
|---|---|
| Интерфейсу не выданы права `fs:*`, `opener:*`, `shell:*`, `http:*`, `dialog:*`. Из плагинов подключён только `tauri-plugin-dialog` — для системных диалогов, которые вызывает ядро из Rust (выбор папки хранилища, подтверждения); его команды интерфейсу недоступны (тест `tauri_and_plugin_commands_are_denied`: `plugin:dialog|open` отклоняется, как и `plugin:event|emit`, `emit_to`, `plugin:path|resolve_directory`, `plugin:window|close`, `plugin:webview|create_webview_window`, `plugin:app|version`, `plugin:fs|read_file`, `plugin:opener|open_url`; сверка с точным текстом отказа Tauri). Из прав Tauri главному окну выданы только `core:event:allow-listen` и `core:event:allow-unlisten` (подписка на события ядра); `core:default` и `core:event:default` не выдаются — в них есть отправка событий из интерфейса (`emit`, `emit_to`), которая не нужна; подключена только эта одна capability | **реализовано:** [src-tauri/capabilities/main.json](../src-tauri/capabilities/main.json), `app.security.capabilities` в [tauri.conf.json](../src-tauri/tauri.conf.json), регистрация плагина в [src-tauri/src/lib.rs](../src-tauri/src/lib.rs), тесты [src-tauri/tests/security_config.rs](../src-tauri/tests/security_config.rs) и [src-tauri/src/commands/acl_tests.rs](../src-tauri/src/commands/acl_tests.rs), ADR 0009 |
| Свои команды ядра перечислены явно; команда вне списка интерфейсу недоступна. Список один — [src-tauri/src/commands/list.rs](../src-tauri/src/commands/list.rs): из него `build.rs` строит `AppManifest::commands`, а ядро — `generate_handler!`; окну выдано `allow-<команда>` ровно на эти 23 команды | **реализовано:** [src-tauri/build.rs](../src-tauri/build.rs), [src-tauri/capabilities/main.json](../src-tauri/capabilities/main.json), тесты [src-tauri/src/commands/acl_tests.rs](../src-tauri/src/commands/acl_tests.rs) на настоящей конфигурации прав: команда, которая есть в обработчике, но не в `AppManifest::commands`, отклоняется проверкой прав Tauri; окну без capability команды недоступны; манифест прав совпадает со списком обработчика; файл разрешений перечисляет ровно эти команды и подписку на события. ADR 0009 |
| Ядро проверяет типы и границы всех параметров и работает только с хранилищем своего окна | **частично (задачи 0.4, 0.4.1):** параметры каждой команды — один объект `request` со строгой схемой (`deny_unknown_fields`, лишнее поле отклоняется); пути — массивом компонентов, а не строкой; пределы проверяются в слое команд до модулей — целей ссылок ≤ 10 000, строка поиска ≤ 256 символов, компонент пути и имя ≤ 255 единиц UTF-16, путь ≤ 64 компонентов, текст заметки ≤ лимита из настроек и ≤ 100 МБ, раскладка ≤ 1 МБ, адрес ссылки ≤ 8 КБ без управляющих символов, диапазоны настроек ([src-tauri/src/commands/limits.rs](../src-tauri/src/commands/limits.rs), тесты там же и `limits_are_checked_before_modules` в [acl_tests.rs](../src-tauri/src/commands/acl_tests.rs)); хранилище выбирается по метке окна, а не по параметру ([src-tauri/src/commands/mod.rs](../src-tauri/src/commands/mod.rs)); страница со стороннего адреса команды вызвать не может (тест `commands_are_denied_from_remote_origin`). Проверка компонентов пути — задача 1.2; события только своему окну — задача 1.7, ADR 0009 |
| `freezePrototype`: прототипы встроенных объектов заморожены; глобальный объект `window.__TAURI__` не создаётся (`withGlobalTauri: false`) | **реализовано:** [src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json), тест [src-tauri/tests/security_config.rs](../src-tauri/tests/security_config.rs), ADR 0009 |
| Isolation pattern: вызовы ядра идут через изолированный iframe с шифрованием | `src-tauri/tauri.conf.json` — выпуск 0.1 (задача 0.3), ADR 0009 |
| Главное окно создаётся в ядре (`WebviewWindowBuilder`), в конфигурации окон нет — это нужно, чтобы задать движку временную папку данных и флаги до создания окна | **реализовано:** [src-tauri/src/lib.rs](../src-tauri/src/lib.rs), тест `security_config.rs`; настройки движка — задача 0.3, ADR 0007 |
| Интерфейс отдаётся из процесса приложения (`http://tauri.localhost` на Windows перехватывается внутри процесса, в сеть не уходит) | Tauri/wry, ADR 0009 |
| Разрешения браузера (камера, микрофон, геолокация, уведомления) отклоняются | `src-tauri/src/security/` — выпуск 0.1, ADR 0009 |
| Шрифты и иконки интерфейса встроены в приложение и грузятся только из его же файлов: `@font-face` с относительными путями, иконки — контуры SVG в коде; внешних адресов и `@import` нет (CSP `default-src 'self'` запрещает их и так) | **реализовано:** [src/theme/fonts.css](../src/theme/fonts.css), [src/shell/Icon.svelte](../src/shell/Icon.svelte), лицензии рядом с файлами, суммы SHA-256 в [docs/dependencies.md](dependencies.md); сценарный тест проверяет загрузку шрифта из файла ([tests/e2e/shell.test.ts](../tests/e2e/shell.test.ts)) |
| Пользователю показываются только тексты из словаря интерфейса: подробности ошибок моста и тексты исключений (в них бывают куски значений — имена заметок, цели ссылок) на экран не выводятся | **реализовано:** [src/i18n/errors.ts](../src/i18n/errors.ts), тесты `src/i18n/errors.test.ts` и `src/shell/Shell.component.test.ts` |
| Раскладка окна пишется только командой ядра `workspace_write` в `.rhizomind/workspace.json` (предел 1 МБ); при чтении интерфейс проверяет формат, версию, глубину, пути и размеры, повреждённая раскладка заменяется раскладкой по умолчанию | **реализовано (интерфейс):** [src/core/layout/layout.ts](../src/core/layout/layout.ts), [src/shell/window.svelte.ts](../src/shell/window.svelte.ts), тесты `src/core/layout/layout.test.ts`; запись на диск — задача 1.2 |
| Инструменты разработчика выключены в выпуске: функция `devtools` крейта `tauri` не включена и запрещена | **реализовано:** [src-tauri/Cargo.toml](../src-tauri/Cargo.toml), [src-tauri/deny.toml](../src-tauri/deny.toml), ADR 0007 |

### Сборка и поставка

| Мера | Где |
|---|---|
| Лицензии всех крейтов совместимы с GPL-3.0 (`cargo-deny`); лицензии npm — с разбором выражений SPDX; список лицензий для ИБ — `cargo-about` | **реализовано:** [src-tauri/deny.toml](../src-tauri/deny.toml) (разрешены только лицензии, встреченные в дереве), [tools/check-licenses/](../tools/check-licenses/) (production-зависимости npm, тесты `licenses.test.ts`); `cargo-about` — задача 3.3, ADR 0009 |
| SBOM (перечень всех зависимостей) — `cargo-cyclonedx` и npm, прикладывается к выпуску | **реализовано:** `npm run sbom` ([tools/sbom/](../tools/sbom/)); приложение к выпуску — задача 3.4 |
| Версии зависимостей фиксируются точно; `Cargo.lock` и `package-lock.json` в репозитории; версия Rust закреплена | **реализовано:** [package.json](../package.json), [src-tauri/Cargo.toml](../src-tauri/Cargo.toml) (`=версия`), [Cargo.lock](../Cargo.lock) (общий для рабочего пространства Cargo), [package-lock.json](../package-lock.json), [rust-toolchain.toml](../rust-toolchain.toml), перечень — [dependencies.md](dependencies.md) |
| GitHub Actions закреплены по SHA коммита, права workflow — только чтение кода | **реализовано:** [.github/workflows/ci.yml](../.github/workflows/ci.yml), ADR 0009 |
| Установщики MSI и NSIS не содержат загрузчика WebView2 (режим `skip`) — установка не обращается к сети; NSIS ставит для текущего пользователя | **реализовано:** `bundle.windows` в [src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json), тест `security_config.rs`, ADR 0007 |
| Порт отладки WebView2 (нужен WebDriver в сценарных тестах) открывается только в тестовой сборке с функцией cargo `webdriver` (`npm run build:e2e`): она передаёт движку флаги msedgedriver из `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`. Выпускные сборки (`build`, `package:portable`, `package:msi`, `package:nsis`) собираются без неё и эту переменную сами не читают. **Но по документации Microsoft загрузчик WebView2 сам учитывает переменные `WEBVIEW2_*` и ключи реестра политик: они переопределяют или дополняют настройки приложения.** Сейчас выпускная сборка от этого не защищена; закрывается удалением `WEBVIEW2_*` из окружения процесса при запуске — задача 0.3, ADR 0007. Наблюдение в CI (WebView2 155 не применил переменную) — не гарантия | [src-tauri/src/lib.rs](../src-tauri/src/lib.rs) (`webdriver_browser_args`), [src-tauri/Cargo.toml](../src-tauri/Cargo.toml) (`[features]`), [package.json](../package.json) |
| Выпускная сборка не прерывается через `abort` при панике (`panic = "unwind"` по умолчанию): на Windows `abort` может запустить отчёт об ошибке Windows с дампом | [src-tauri/Cargo.toml](../src-tauri/Cargo.toml) (`[profile.release]` без `panic = "abort"`) |

## Безопасность по выпускам

| Выпуск | Что добавляется для ИБ |
|---|---|
| 0.1 | Все инварианты Secure-редакции, меры WebView2, граница ядро — интерфейс, проверки зависимостей, установщики MSI и NSIS. Ревью безопасности. |
| 0.2 | Индекс в `.rhizomind/`; проверка, что индекс не покидает хранилище. Ревью безопасности. |
| 0.3 | SQLite без временных файлов вне хранилища; вложения и буфер обмена в пределах контура хранилища. Ревью безопасности. |
| 0.4 | Раскладка в `.rhizomind/`. Ревью безопасности. |
| 0.5 | Свои темы, журнал ошибок, пакет документов для ИБ, полный аудит безопасности. |

## Как проверить самостоятельно

- **Сетевой монитор** (Wireshark или брандмауэр Windows с журналом): запустить приложение, пройти обычную работу. Процесс `rhizomind.exe` не устанавливает соединений; запущенные им процессы `msedgewebview2.exe` обращаются только к службам Microsoft, описанным выше.
- **Process Monitor** (фильтр по `rhizomind.exe` и дочерним `msedgewebview2.exe`): вне хранилища запись только в `%APPDATA%\Rhizomind\vaults.json` и во временную папку `rhizomind-*`; после выхода временной папки нет.
- **`cargo deny check`** в папке `src-tauri/`: проверка лицензий, запрещённых сетевых крейтов и функций.
- **`cargo tree -e normal --target x86_64-pc-windows-msvc`** (и для других целей): в выводе нет `reqwest`, `hyper`, `h2`, `rustls`, `native-tls`, `openssl`, `socket2`, `tungstenite`.
- Прочитать файлы из таблиц выше: весь код открыт.

## Известные ограничения

- Сборки пока не подписаны цифровой подписью: Windows показывает предупреждение SmartScreen при запуске установщика. Для корпоративного распространения нужна подпись кода (Windows Authenticode, Apple Developer ID).
- Заметки хранятся без шифрования: защита данных на диске обеспечивается средствами ОС (BitLocker, FileVault, LUKS). Шифрование появится только по требованию ИБ.
- WebView2 — компонент Windows: его обновления и обязательная диагностика управляются Microsoft и службой ИТ, а не приложением (см. «Честно о WebView2»).
- Установка NSIS для текущего пользователя кладёт файлы программы в `%LOCALAPPDATA%\Programs\Rhizomind`: это исполняемые файлы, а не данные приложения.
- macOS и Linux: меры для WKWebView и WebKitGTK проверяются при переходе на эти ОС.
