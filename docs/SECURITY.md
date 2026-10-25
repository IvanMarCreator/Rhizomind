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
| Независимая проверка `cargo tree` по каждой десктопной цели (Windows, macOS, Linux): сетевых крейтов нет | **реализовано:** [tools/check-cargo-tree/](../tools/check-cargo-tree/) (`cargo tree -e normal --target <цель>` для `x86_64`/`aarch64` Windows и macOS и `x86_64` Linux; кроме имён ловит `tokio`/`mio` с функцией `net`), тесты `tree.test.ts`, ADR 0009 |
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
| Доступ только внутри открытого хранилища: путь от интерфейса — относительный, по компонентам; каждый компонент проверяется: пустой, `.`, `..`, разделители `/` и `\` (а с ними `\\?\`, `\\.\` и абсолютные пути), `:` (буквы дисков, потоки NTFS `имя:поток`), NUL и управляющие символы, `* ? " < > |`, точка или пробел в конце — отклоняются (`invalid_path`) | **реализовано (задача 1.2):** `check_component` в [src-tauri/src/vault/contour.rs](../src-tauri/src/vault/contour.rs), тесты [vault/tests/contour.rs](../src-tauri/src/vault/tests/contour.rs) |
| Зарезервированные имена Windows (`CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`, `COM1`–`COM9`, `LPT1`–`LPT9`, `COM¹`–`LPT³`, в том числе с расширением: `nul.md`) отклоняются в каждом компоненте пути при любом обращении, не только при создании | **реализовано:** [contour.rs](../src-tauri/src/vault/contour.rs); правила имён для интерфейса — `src-tauri/src/names/`, ADR 0009 |
| Корень хранилища канонизируется один раз при открытии (`dunce::canonicalize`); итоговый путь каждого обращения начинается с корня | **реализовано:** `Vault::open` в [vault/mod.rs](../src-tauri/src/vault/mod.rs), `Contour::resolve` в [contour.rs](../src-tauri/src/vault/contour.rs) |
| Символические ссылки и точки соединения NTFS не используются для выхода за контур: каждый промежуточный компонент и сам элемент проверяются `symlink_metadata` (на Windows `is_symlink()` истинен и для точек соединения), ссылка → `outside_vault`; дерево ссылки не показывает и не обходит. Тесты: ссылки на папку и на файл, вглубь, на служебную папку; точки соединения (`junction`) — в CI на Windows | **реализовано:** [contour.rs](../src-tauri/src/vault/contour.rs), обход дерева в [vault/fsops.rs](../src-tauri/src/vault/fsops.rs), тесты `symlinks_never_leave_the_vault`, `ntfs_junctions_never_leave_the_vault` в [tests/contour.rs](../src-tauri/src/vault/tests/contour.rs) |
| Проверка итогового пути **по дескриптору** после открытия файла или папки: `GetFinalPathNameByHandleW` на Windows, `/proc/self/fd/<n>` на Linux. Ловит подмену компонента ссылкой между проверкой и открытием (гонка «проверил — открыл») и обращение через короткое имя 8.3: путь вне корня или в `.rhizomind/` → `outside_vault` | **реализовано:** `Contour::check_opened` в [contour.rs](../src-tauri/src/vault/contour.rs) — чтение и запись заметки, создание, переименование, перемещение, корзина, служебные файлы; остаточный риск — в «Известных ограничениях» |
| Служебная папка `.rhizomind/` недоступна обычным командам: первый компонент сравнивается по ключу имени (`.rhizomind`, `.Rhizomind`, `.RHIZOMIND`), компоненты вида короткого имени 8.3 (`RHIZOM~1`, `ПРОЕКТ~2.MD`: основа до 8 символов с `~цифры`, расширение до 3) отклоняются, итоговый путь по дескриптору не может лежать в `.rhizomind/`. Длинные имена с тильдой (`Версия~2 итог.md`) разрешены: коротким именем они быть не могут | **реализовано:** `is_service_dir`, `looks_like_short_name`, `check_real_path` в [contour.rs](../src-tauri/src/vault/contour.rs), тест `service_folder_is_unreachable_by_any_spelling` |
| `.rhizomind/` и `.rhizomind/tmp/` при открытии хранилища проверяются: не ссылки и не точки соединения, итоговый путь внутри корня; служебный файл-ссылка (`settings.json`, `workspace.json`) не читается и не заменяется | **реализовано:** `Vault::open`, `read_service_file`, `write_service_file` в [vault/mod.rs](../src-tauri/src/vault/mod.rs), тесты в [tests/service.rs](../src-tauri/src/vault/tests/service.rs) |
| Атомарная запись: текст пишется во временный файл в `.rhizomind/tmp/` (`tempfile`), сбрасывается на диск (`sync_all`), затем заменяет заметку — на Windows через `ReplaceFileW` (дата создания, атрибуты и права заменяемого файла сохраняются), на других ОС через `rename` с переносом прав. Сбой посреди записи оставляет заметку целой; остатки после аварийного завершения убираются при открытии хранилища | **реализовано:** [vault/atomic.rs](../src-tauri/src/vault/atomic.rs), тесты `failure_in_the_middle_of_a_write_leaves_the_note_whole`, `leftovers_of_an_interrupted_write_are_removed_on_open`, `write_keeps_the_creation_date` (Windows) в [tests/write.rs](../src-tauri/src/vault/tests/write.rs) |
| Запись сверяется с версией файла на диске (хэш `blake3` и размер); внешние изменения не затираются: версия с диска сохраняется копией «Имя (внешняя версия ГГГГ-ММ-ДД ЧЧ-ММ).md» **до** замены заметки, без затирания существующих файлов (`persist_noclobber`), с номером при повторе. Неизменённый текст не пишется (байт в байт); BOM и преобладающие переносы сохраняются | **реализовано:** `Vault::write_note` в [vault/mod.rs](../src-tauri/src/vault/mod.rs), [vault/text.rs](../src-tauri/src/vault/text.rs), тесты в [tests/write.rs](../src-tauri/src/vault/tests/write.rs) |
| Переименование и перемещение не затирают существующий элемент: `MoveFileExW` без `MOVEFILE_REPLACE_EXISTING` (Windows), `renameat2(RENAME_NOREPLACE)` (Linux); совпадение имён — по ключу имени; папку нельзя переместить в себя | **реализовано:** [vault/fsops.rs](../src-tauri/src/vault/fsops.rs), тесты в [tests/ops.rs](../src-tauri/src/vault/tests/ops.rs) |
| Ограничение размера редактируемого файла (по умолчанию 10 МБ, настройка хранилища): больше лимита — открывается только для чтения; не UTF-8 — только чтение, запись отклоняется (`read_only`); у `note_write` — жёсткий потолок 100 МБ | **реализовано:** `Vault::read_note`, `Vault::write_note` в [vault/mod.rs](../src-tauri/src/vault/mod.rs), потолок — [commands/note.rs](../src-tauri/src/commands/note.rs) |
| Удаление в корзину ОС (крейт `trash`, в отдельном потоке). На Windows корзина считается доступной только на несъёмном локальном томе (`GetDriveTypeW` = `DRIVE_FIXED`): на флешке и в сетевой папке оболочка Windows удалила бы файл безвозвратно сама. Корзины нет или она отказала — ядро спрашивает подтверждение и удаляет безвозвратно только при согласии, после повторной проверки пути по контуру | **реализовано:** `Vault::trash` в [vault/mod.rs](../src-tauri/src/vault/mod.rs), `os_trash` в [vault/fsops.rs](../src-tauri/src/vault/fsops.rs), тесты в [tests/ops.rs](../src-tauri/src/vault/tests/ops.rs); окно подтверждения — задача 1.7 (до неё безвозвратное удаление отклоняется) |
| Свои записи ядра регистрируются по хэшу и размеру до замены файла — наблюдатель не примет их за внешнее изменение | **реализовано:** [vault/own_writes.rs](../src-tauri/src/vault/own_writes.rs); подключение к наблюдателю — задачи 1.4, 1.7 |
| Ошибки для интерфейса — только машинный код, без путей и текста ОС | **реализовано:** `io_error` в [vault/mod.rs](../src-tauri/src/vault/mod.rs) |
| Слежение за папкой через уведомления ОС (`notify`, без сетевого кода); своя запись отличается от внешней по версии файла | `src-tauri/src/watcher/` — выпуск 0.1, ADR 0006 |
| Вложения (в том числе из буфера обмена) сохраняются только внутри хранилища | выпуск 0.3 |

### Изоляция интерфейса

| Мера | Где |
|---|---|
| Интерфейсу не выданы права `fs:*`, `opener:*`, `shell:*`, `http:*`, `dialog:*`. Из плагинов подключён только `tauri-plugin-dialog` — для системных диалогов, которые вызывает ядро из Rust (выбор папки хранилища, подтверждения); его команды интерфейсу недоступны (тест `tauri_and_plugin_commands_are_denied`: `plugin:dialog|open` отклоняется, как и `plugin:event|emit`, `emit_to`, `plugin:path|resolve_directory`, `plugin:window|close`, `plugin:webview|create_webview_window`, `plugin:app|version`, `plugin:fs|read_file`, `plugin:opener|open_url`; сверка с точным текстом отказа Tauri). Из прав Tauri главному окну выданы только `core:event:allow-listen` и `core:event:allow-unlisten` (подписка на события ядра); `core:default` и `core:event:default` не выдаются — в них есть отправка событий из интерфейса (`emit`, `emit_to`), которая не нужна; подключена только эта одна capability | **реализовано:** [src-tauri/capabilities/main.json](../src-tauri/capabilities/main.json), `app.security.capabilities` в [tauri.conf.json](../src-tauri/tauri.conf.json), регистрация плагина в [src-tauri/src/lib.rs](../src-tauri/src/lib.rs), тесты [src-tauri/tests/security_config.rs](../src-tauri/tests/security_config.rs) и [src-tauri/src/commands/acl_tests.rs](../src-tauri/src/commands/acl_tests.rs), ADR 0009 |
| Свои команды ядра перечислены явно; команда вне списка интерфейсу недоступна. Список один — [src-tauri/src/commands/list.rs](../src-tauri/src/commands/list.rs): из него `build.rs` строит `AppManifest::commands`, а ядро — `generate_handler!`; окну выдано `allow-<команда>` ровно на эти 23 команды | **реализовано:** [src-tauri/build.rs](../src-tauri/build.rs), [src-tauri/capabilities/main.json](../src-tauri/capabilities/main.json), тесты [src-tauri/src/commands/acl_tests.rs](../src-tauri/src/commands/acl_tests.rs) на настоящей конфигурации прав: команда, которая есть в обработчике, но не в `AppManifest::commands`, отклоняется проверкой прав Tauri; окну без capability команды недоступны; манифест прав совпадает со списком обработчика; файл разрешений перечисляет ровно эти команды и подписку на события. ADR 0009 |
| Ядро проверяет типы и границы всех параметров и работает только с хранилищем своего окна | **частично (задачи 0.4, 0.4.1):** параметры каждой команды — один объект `request` со строгой схемой (`deny_unknown_fields`, лишнее поле отклоняется); пути — массивом компонентов, а не строкой; пределы проверяются в слое команд до модулей — целей ссылок ≤ 10 000, строка поиска ≤ 256 символов, компонент пути и имя ≤ 255 единиц UTF-16, путь ≤ 64 компонентов, текст заметки ≤ 100 МБ (лимит из настроек — при открытии, задача 1.2), раскладка ≤ 1 МБ, адрес ссылки ≤ 8 КБ без управляющих символов, диапазоны настроек ([src-tauri/src/commands/limits.rs](../src-tauri/src/commands/limits.rs), тесты там же и `limits_are_checked_before_modules` в [acl_tests.rs](../src-tauri/src/commands/acl_tests.rs)); хранилище выбирается по метке окна, а не по параметру ([src-tauri/src/commands/mod.rs](../src-tauri/src/commands/mod.rs)); страница со стороннего адреса команды вызвать не может (тест `commands_are_denied_from_remote_origin`). Проверка компонентов пути — реализована в задаче 1.2 (см. «Файлы»); события только своему окну — задача 1.7, ADR 0009 |
| `freezePrototype`: прототипы встроенных объектов заморожены; глобальный объект `window.__TAURI__` не создаётся (`withGlobalTauri: false`) | **реализовано:** [src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json), тест [src-tauri/tests/security_config.rs](../src-tauri/tests/security_config.rs), ADR 0009 |
| Isolation pattern: вызовы ядра идут через изолированный iframe с шифрованием | `src-tauri/tauri.conf.json` — выпуск 0.1 (задача 0.3), ADR 0009 |
| Главное окно создаётся в ядре (`WebviewWindowBuilder`), в конфигурации окон нет — это нужно, чтобы задать движку временную папку данных и флаги до создания окна | **реализовано:** [src-tauri/src/lib.rs](../src-tauri/src/lib.rs), тест `security_config.rs`; настройки движка — задача 0.3, ADR 0007 |
| Интерфейс отдаётся из процесса приложения (`http://tauri.localhost` на Windows перехватывается внутри процесса, в сеть не уходит) | Tauri/wry, ADR 0009 |
| Разрешения браузера (камера, микрофон, геолокация, уведомления) отклоняются | `src-tauri/src/security/` — выпуск 0.1, ADR 0009 |
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
- **Жёсткие ссылки (hard links).** Жёсткая ссылка — второе имя того же файла; ссылку внутри хранилища на файл вне его контуром не отличить: для ОС это обычный файл хранилища. Создать её может только тот, у кого уже есть право записи в обе папки (тот же пользователь или администратор). Rhizomind жёстких ссылок не создаёт; запись через замену файла (`ReplaceFileW`, `rename`) разрывает связь и меняет только имя внутри хранилища, исходный файл вне хранилища не изменяется.
- **Гонка «проверил — открыл».** Чтение и запись проверяют итоговый путь по уже открытому дескриптору. Замена файла (`ReplaceFileW`, `rename`), переименование, перемещение и удаление выполняются по пути: если в доли секунды между проверкой и операцией процесс с правами пользователя подменит промежуточную папку ссылкой, операция может попасть по ссылке. Это требует вредоносной программы, уже работающей от имени пользователя (ей контур не нужен — у неё и так есть доступ к его файлам). Второй слой (`cap-std`, операции относительно дескриптора папки) — возможное улучшение (исследование крейтов).
- **macOS:** проверка итогового пути по дескриптору пока заменена проверкой по пути (`canonicalize`); дата создания при записи не переносится.
- **Linux:** дата создания заметки после записи может сдвинуться (замена через `rename`), поэтому «самая старая» из заметок с одним именем может смениться после правки (ADR 0006, уточнение 25.10.2026).
