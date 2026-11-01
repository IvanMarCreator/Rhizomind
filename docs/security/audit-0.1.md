# Аудит безопасности выпуска 0.1 (задача 3.3)

Дата: 01.11.2026. Ревьюер безопасности (агент). Код: `main` 5b34010. Основания: инварианты 1–5 (`CLAUDE.md`), спецификация 0.1 (С6, раздел 9, решения раздела 12), ADR 0007, 0009, `docs/SECURITY.md`.

**Вердикт.** В коде и автоматических проверках нарушений инвариантов не найдено: в программе нет сетевых крейтов и сетевого кода интерфейса, права окон и фильтр изолированного iframe минимальны, внешняя ссылка уходит в ОС только после подтверждения в окне ядра, движок работает в InPrivate во временной папке, дампы в Microsoft не уходят. **Выпуск блокирует одно — не выполнен ручной сетевой аудит и проверка Process Monitor на чистой Windows** (критерии 3 и 4 раздела 9 спецификации; без них флаги против фоновой сети — не гарантия). Сценарий и помощник для владельца готовы: [network-audit.md](network-audit.md), [network-audit.ps1](network-audit.ps1). Остальные находки — описание в `SECURITY.md` (исправлено в этой задаче), одно решение владельца (контекстное меню движка) и замечания.

## 1. Что проверено командами

Все команды — на копии `task/3.3-security-audit` от `main` 5b34010, Linux, `CARGO_TARGET_DIR` общий.

| Команда | Результат |
|---|---|
| `npm run check:deny` | `advisories ok, bans ok, licenses ok, sources ok` — для программы и для генератора |
| `npm run check:cargo-tree` | Windows x86_64/aarch64 — 168 крейтов, macOS — 172, Linux — 196; **сетевых нет (пусто)** по всем пяти целям; крейтов только для тестов в программе нет |
| `npm run check:network` | `сетевой код не найден (src, src-isolation, dist); по разобранным исключениям пропущено находок: 9` (9 = число записей `allowlist.json`, копия в `SECURITY.md`) |
| `npm run check:licenses` | 32 production-зависимости npm, все лицензии совместимы с GPL-3.0 (MIT, Apache-2.0, `Apache-2.0 OR MIT`) |
| `npm run sbom` (после исправления, цель `x86_64-pc-windows-msvc`) | SBOM ядра — 266 компонентов, CycloneDX 1.5, нет `webkit2gtk`, есть `webview2-com`, нет `reqwest`/`hyper`/`rustls`/`h2`/`socket2`/`openssl`; wry указан своей копией (`download_url=file://../vendor/wry`); лицензии крейтов: 0BSD, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-3-Clause, BSL-1.0, CC0-1.0, GPL-3.0-only (сам Rhizomind), MIT, MIT-0, MPL-2.0, Unicode-3.0, Unlicense, Zlib; без лицензии — 0 |
| Сверка `vendor/wry` с архивом crates.io | SHA-256 архива `759c50dd…cf0e9` — как в `RHIZOMIND-PATCH.md`; `diff -r` — отличаются только `src/webview2/mod.rs` (4 строки: комментарий и `options.set_is_custom_crash_reporting_enabled(true)`) и добавленный `RHIZOMIND-PATCH.md` |

Сценарные тесты Windows (тест следов, `webview2_runtime.rs`, `browser-exit`, `session-end`) в облачной копии не запускались — они идут в CI на `windows-latest` (шаг «Сценарный тест (Windows)»), их результат берётся из CI `main`.

## 2. Сверка с инвариантами и С6

Статусы: **есть** — реализовано и доказано автоматической проверкой; **есть, ждёт ручного аудита** — реализовано, окончательное доказательство — ручной аудит на чистой Windows; **частично** — есть расхождение (раздел 3).

### Инвариант 1. Доказуемо нет сети

| Требование | Где | Доказательство | Статус |
|---|---|---|---|
| Нет сетевых крейтов и плагинов Tauri (`http`, `updater`, `websocket`, `upload`, `localhost`, `shell`), нет `tokio/net`, `mio/net` | `src-tauri/deny.toml:54` (`[bans] deny`), `:70`, `:74` | `cargo deny` и `check:cargo-tree` в `npm run verify` (CI, шаг «Проверки»); выводы выше | есть |
| Свой код Rust не открывает сокетов и не разрешает имён | `clippy.toml:6–7` и далее (`socket_addrs`, `to_socket_addrs`, `TcpStream`, `UdpSocket`, `TcpListener`) | `cargo clippy -D warnings` в `check:rust` | есть |
| Интерфейс без сетевого кода (исходники и собранный `dist/`) | `tools/check-no-network/` | `check:network`; тесты `scan.test.ts`, `allowlist.test.ts`; 9 разобранных исключений (решение 52) | есть |
| CSP без внешних хостов | `src-tauri/tauri.conf.json:17` | `security_config.rs`; сценарный `secure-shell.test.ts` (`fetch` наружу отклонён) | есть (замечание З2) |
| Запрет переходов, новых окон, загрузок, разрешений браузера | `src-tauri/src/lib.rs:224–238` (`build_window`), `security/mod.rs:175` (`is_allowed_navigation`) | модульные тесты правила адресов, `secure-shell.test.ts` | есть |
| Флаги движка против фоновой сети | `src-tauri/src/security/mod.rs:29–50` | тест точной строки (`:198`), командная строка процесса браузера в `traces.test.ts`; **действие флагов** — только ручным аудитом | есть, ждёт ручного аудита (Б1) |
| SmartScreen выключен | `security/windows.rs:53` (`disable_smartscreen`), вызов `lib.rs:261` | `webview2_runtime.rs` читает значение у живого движка | есть (замечание З1) |
| Отчёты о падениях не уходят в Microsoft | `vendor/wry/src/webview2/mod.rs:357–360`, `[patch.crates-io]` в корневом `Cargo.toml` | `wry_comes_from_own_patched_copy_of_the_same_version`, ключ `--edge-webview-disable-crash-reporting=1` в `webview2_runtime.rs`, `traces.test.ts`; сверка копии выше | есть |
| Встроенного браузера, загрузок, телеметрии, автообновлений, плагинов нет | `deny.toml` (`updater` запрещён), `lib.rs:226–228`, нет регистрации плагинов кроме `dialog` | `deny_bans_devtools_on_every_crate_and_asset_protocol`, `tauri_and_plugin_commands_are_denied` | есть |
| Установщики без сети | `tauri.conf.json` `bundle.windows.webviewInstallMode = skip` | `security_config.rs` | есть |
| Внешняя ссылка — только после подтверждения (д) | `commands/settings.rs:233` (`open_external`) → `external_links/mod.rs:90` (`open_checked`), `:112`; ОС — `external_links/os.rs` (`ShellExecuteExW` с классом протокола) | `without_confirmation_address_does_not_reach_os`, `other_schemes_are_rejected_without_dialog`, `dialog_shows_exactly_the_string_sent_to_os`, `capabilities_grant_no_opener_shell_fs_or_dialog`; других вызовов запуска программ в ядре нет (поиск `ShellExecute`, `Command::new`, `open::that` — только `external_links/os.rs`) | есть |
| WebView2 Evergreen: обязательная диагностика и ECS | `SECURITY.md`, «Честно о WebView2», «Политики для службы ИТ» | описано; неустранимо приложением (ADR 0007) | есть; список ожидаемых адресов добавлен (Р7) |

### Инвариант 2. Контур хранилища и учёт записей вне него

| Требование | Где | Доказательство | Статус |
|---|---|---|---|
| Файлы — только внутри открытого хранилища (компоненты пути, ссылки, точки соединения, 8.3, дескриптор) | `src-tauri/src/vault/contour.rs`, `vault/mod.rs`, `commands/mod.rs` (`checked_path`) | тесты `vault/tests/contour.rs`, `hardening.rs`, `core_tests.rs` (`window_cannot_reach_the_vault_of_another_window`) | есть (ограничение «гонка проверил — открыл» описано) |
| Служебные данные — в `.rhizomind/` | `vault/mod.rs` (`read_service_file`, `write_service_file`) | `vault/tests/service.rs` | есть |
| Записи вне хранилища перечислены в `SECURITY.md` «Записи вне хранилища» (б) | `docs/SECURITY.md` | сверка ниже | частично → исправлено: **не было корзины ОС** (Р2) |
| Не в сеть и не в синхронизируемые места (а) | `registry/mod.rs` (`dirs::data_local_dir()`), окно выбора папки `dialogs/folder_windows.rs` | `profile_file_is_in_local_app_data_not_roaming`, `chosen_folder_path_does_not_reach_roaming_profile` | есть, с принятыми исключениями в Roaming: 61, 65; открыт ConsentStore (Р5) |
| Автоматическая проверка следов ловит новые записи (в) | `tests/e2e/traces.test.ts`, `tools/traces/registry.ts` | тест следов в CI на Windows | частично: ловит файлы в трёх папках профиля и реестр по меткам; описание было шире реального (Р6, исправлено) |
| Имена и содержимое заметок вне хранилища не пишутся | `registry/mod.rs` (только имена и пути хранилищ) | `file_holds_only_names_paths_and_window_state`; метки данных в тесте следов | есть; единственное исключение — корзина ОС по действию пользователя (Р2) |
| Заголовок окна с именем заметки (решение 82) (е) | `windows/mod.rs:111` (`window_title`), `commands/window.rs:53` (`window_shown_note`), фильтр `src-isolation/index.js` | `vault_title_*`, `window_title_*`, `shown_note_is_checked_against_the_vault_of_the_window`, `release.test.ts` | есть; описание в `SECURITY.md` честное: программа ничего не пишет, ОС и её функции (Recall, Timeline) видят заголовок. Добавлена политика ИТ для Recall (З8) |
| Служебные файлы WebView2 — во временной папке с удалением | `webview_traces/mod.rs:62` (`create_data_dir`), `:78` (`sweep_leftovers`), `:199` (`remove_after_exit`) | тесты модуля, `webview2_runtime.rs`, `traces.test.ts` | есть |

### Инвариант 3. Движок ничего не сохраняет

| Требование | Где | Доказательство | Статус |
|---|---|---|---|
| InPrivate, окно — только после подтверждения движком | `security/mod.rs` (`incognito`, `in_private_check`), `lib.rs:254–280` | `incognito_is_on_in_every_build_except_linux_webdriver`, `window_opens_only_when_engine_confirms_in_private`, `webview2_runtime.rs` | есть |
| Переменные `WEBVIEW2_*` не действуют | `security/mod.rs:157`, вызов `lib.rs:84` | `traces.test.ts` (порт отладки закрыт, папка не создана) | есть |
| Дампов нет | правка wry; `security/windows.rs:28` (`limit_error_reporting`), вызов `lib.rs:86`; `panic = "unwind"` | `browser-exit.test.ts` (новых отчётов WER нет), `security_config.rs` | есть, с оговорками: локальный дамп Crashpad во временной папке до выхода (описан), WER-отчёты о зависании — политика ИТ (З6, З7) |
| Журнал ошибок без содержимого заметок, с ограничением | — | журнала нет до 0.5; stderr — строки без имён | не применимо в 0.1 |
| Инструменты разработчика выключены | `src-tauri/Cargo.toml`, `deny.toml` | `deny_bans_devtools_on_every_crate_and_asset_protocol`, `release-binary.test.ts` | есть |

### Инвариант 4. Без потери данных

Сверено по `SECURITY.md` «Файлы» и тестам: атомарная запись, сверка версии до и после временного файла, копия «внешняя версия» без затирания, корзина ОС с проверкой типа тома, подтверждение ядром на диске без корзины, закрытие окна и конец сеанса с ожиданием записи, решение 66. Нарушений не найдено. Открытый вопрос — диск `DRIVE_FIXED` с отключённой корзиной: библиотека `trash` 5.2.9 передаёт `FOF_WANTNUKEWARNING` (`trash-5.2.9/src/windows.rs:97`), то есть Windows должна сама предупредить о безвозвратном удалении; это проверяется вручную (шаг в [network-audit.md](network-audit.md), «Без скрипта»).

### Инвариант 5 и С6 «Сборка»

| Требование | Где | Статус |
|---|---|---|
| Каждая мера — в `SECURITY.md` со ссылкой на файл | `docs/SECURITY.md` | есть; найдены неточности описания — исправлены (раздел 3) |
| Лицензии: `cargo-deny`, проверка npm с SPDX, **`cargo-about`** | `deny.toml`, `tools/check-licenses/` | частично: `cargo-about` не внедрён; список лицензий крейтов теперь даёт `npm run sbom` через `cargo deny list` (Р8) |
| SBOM (`cargo-cyclonedx` + npm) | `tools/sbom/cli.ts` | частично → исправлено: цель была неявной (Р1); в CI не собирается (Р9, задача 3.4) |
| Actions по SHA, права только чтение | `.github/workflows/ci.yml` | есть |
| Скрипты установки npm выключены | `.npmrc` | есть |

### Права окон и изоляционный фильтр (г)

- Окна хранилищ (`capabilities/main.json`): 24 своих команды + `core:event:allow-listen`/`allow-unlisten`; окно управления (`capabilities/manager.json`): 7 команд группы «Хранилища», без прав Tauri. Каждое право используется интерфейсом; лишних нет; `core:default`, `core:window:*`, `core:webview:*`, `dialog:*`, `fs:*`, `opener:*`, `shell:*`, `http:*` не выданы. Состав сверяют `main_capability_allows_exactly_vault_window_commands_and_event_listening`, `manager_capability_allows_exactly_manager_commands`, второй рубеж ядра — `core_refuses_manager_commands_from_other_windows`.
- Фильтр `src-isolation/index.js`: 28 команд из `commands/list.rs`, `plugin:event|listen` — только 7 событий ядра и только с целью `WebviewWindow` и непустой меткой, `unlisten`; остальное подменяется на несуществующую команду. Расхождение со списком ядра ловит `filter.test.ts`. Фильтр не различает окна (это делают capability и ядро) — так задумано.
- `withGlobalTauri: false`, `freezePrototype: true`, isolation pattern включён (`tauri.conf.json`).

## 3. Расхождения

### Блокирует выпуск

| № | Что | Почему блокирует | Что сделать |
|---|---|---|---|
| Б1 | Ручной сетевой аудит и проверка Process Monitor на чистой Windows **не проведены**; действие флагов `--disable-background-networking`, `--disable-component-update`, `--disable-domain-reliability`, `--no-pings`, `--host-resolver-rules` не подтверждено | критерии приёмки 3 и 4 (спецификация, раздел 9), ADR 0007: «флаг без подтверждённого действия гарантией не называется» | владелец проходит [network-audit.md](network-audit.md) с [network-audit.ps1](network-audit.ps1) на второй машине до задачи 3.4; оркестратор переносит итог сюда (раздел 5), в `SECURITY.md` («Сеть WebView2», «Ожидаемые обращения к Microsoft», «Известные ограничения») и в ADR 0007. `--no-pings` подтвердить нечем (в интерфейсе нет `<a>`) — в `SECURITY.md` уже записан как запас, не гарантия |

### Исправить до выпуска

| № | Что | Где | Статус |
|---|---|---|---|
| Р1 | SBOM ядра строился для цели машины сборки (`cargo-cyclonedx` по умолчанию берёт цель `rustc -vV`): на Linux в перечне были `webkit2gtk`/`soup3` и не было `webview2-com`/`windows` | `tools/sbom/cli.ts` | **исправлено в этой задаче:** цель `x86_64-pc-windows-msvc` явно (`--target` для другой), CycloneDX 1.5, без сети (`CARGO_NET_OFFLINE=true`, `npm --offline`), плюс `release/licenses-rust.json` |
| Р2 | Корзина ОС (`$Recycle.Bin` — имя, исходный путь и текст удалённой заметки) не была в «Записях вне хранилища»; тест следов этот сценарий не проходит | `docs/SECURITY.md` | **исправлено:** строка добавлена; проверка — ручной аудит, шаг 6 |
| Р3 | «Всего 27 команд» — с `window_shown_note` (2.12) их 28 | `docs/SECURITY.md`, «Изоляция интерфейса» (две строки) | **исправлено** |
| Р4 | **Контекстное меню движка включено** (`SetAreDefaultContextMenusEnabled(true)` — значение wry по умолчанию, `vendor/wry/src/webview2/mod.rs:653`). По документации WebView2 в нём бывают «Поделиться» (общий доступ Windows — передача текста в другие приложения), «Печать», «Сохранить как», «Веб-снимок» и др. Это не сеть Rhizomind, но каналы вывода данных, которые `SECURITY.md` не описывает; и расходится с решением 94 (Ctrl+P в окне ничего не делает, а «Печать» в меню есть). Загрузку «Сохранить как» гасит `on_download`, но это не проверялось | `src-tauri/src/lib.rs` (`harden_platform_webview`) | **решение владельца** (вопрос 1 ниже); точные пункты меню записывает ручной аудит, шаг 7 |
| Р5 | Запись `CapabilityAccessManager\ConsentStore\…\NonPackaged` (перемещаемый профиль) в «Записях вне хранилища» помечена «не проверено»; решения владельца по ней нет (в отличие от 61, 65); в `KNOWN_KEYS` теста следов её нет | `docs/SECURITY.md`, `tools/traces/registry.ts` | подтвердить ручным аудитом (скрипт помечает ключ); затем решение владельца (вопрос 2 ниже) и строка в `KNOWN_KEYS` |
| Р6 | `SECURITY.md` обещал «новая запись без строки здесь валит тест следов» — на деле изменения HKCU без меток Rhizomind печатаются как фон, а записи вне `%LOCALAPPDATA%`, `%APPDATA%`, `%TEMP%` и сценарии вне теста (окно выбора папки, корзина, ссылки, настройки) не проверяются | `docs/SECURITY.md`, «Записи вне хранилища» | **исправлено:** описано, что тест ловит и что нет; пробелы закрывает ручной аудит |
| Р7 | Критерий 3 сверяет обращения «со службами Microsoft, описанными в `SECURITY.md`», но списка служб и адресов не было | `docs/SECURITY.md` | **исправлено:** раздел «Ожидаемые обращения к Microsoft» (предварительный, уточняется аудитом) |
| Р8 | `cargo-about` из С6 и ADR 0009 не внедрён | ADR 0009, `SECURITY.md` | сделано заменой: список лицензий — `cargo deny list` в `npm run sbom` (инструмент уже закреплён в CI). Оркестратору — принять отступление и дописать в ADR 0009 |
| Р9 | SBOM не собирается в CI — инструмент может незаметно сломаться к выпуску | `.github/workflows/ci.yml` | **исправлено (задача 3.3а):** шаг «SBOM (Windows)» — `cargo install --locked cargo-cyclonedx@0.5.9` (сеть только у машины сборки, инструмент в программу не входит) и `npm run sbom`, артефакт `rhizomind-sbom` с тремя файлами `release/`. Приложение к выпуску — задача 3.4 |

### Замечания

| № | Что | Рекомендация |
|---|---|---|
| З1 | Отказ выключить SmartScreen только пишется в stderr, окно открывается (`lib.rs:261–262`); InPrivate при отказе окно не открывает | **исправлено (задача 3.3а):** общее правило `security::engine_check` — не принята любая защитная настройка (SmartScreen, ускорители браузера; меню движка из 2.19 добавляется строкой в тот же список в `harden_platform_webview`) — окно не открывается, сообщение как для InPrivate; тест `window_opens_only_when_every_engine_setting_is_applied` |
| З2 | В CSP `img-src` есть `asset: http://asset.localhost`, хотя протокол `asset:` запрещён | **исправлено (задача 3.3а):** убрано из `csp` и `devCsp`, ADR 0009, `SECURITY.md`; тест `csp_does_not_mention_disabled_asset_protocol` |
| З3 | `spellcheck="false"` стоит в редакторе, поле имени, быстром переходе, создании хранилища; нет — в поиске горячих клавиш, фильтре папок, числовых полях настроек | **исправлено (задача 3.3а):** атрибут во всех полях; тест `src/spellcheck.test.ts` разбирает разметку всех `.svelte` и валится на поле без него. По Microsoft проверка Edge/WebView2 — локальная служба Windows Spellcheck, но без языкового пакета движок переходит на Hunspell со скачиваемыми словарями — поэтому выключено везде; что словари не скачиваются — ручной аудит, шаг 7 |
| З4 | ADR 0007 перечисляет три флага против фоновой сети, в коде и `SECURITY.md` их пять (`--disable-domain-reliability`, `--no-pings`) | **исправлено (задача 3.3а):** ADR 0007 перечисляет все пять со ссылкой на `security/mod.rs`; итог ручного аудита по каждому флагу дописывается после Б1 |
| З5 | `docs/dependencies.md`: у `@codemirror/view` устаревшая фраза «шесть мест … решение за владельцем» — решение 52 принято, разобранных исключений 9; не перечислены 14 production-зависимостей npm от `svelte` (компилятор: `acorn`, `esrap`, `magic-string` и др.), которые видит `check:licenses` | **исправлено (задача 3.3а):** фраза заменена ссылкой на решение 52; перечислены все 18 зависимостей `svelte` из `package-lock.json`; добавлены пропущенные `dirs` 7.0.0 (программа) и функция `windows` для тестов; у `cargo-cyclonedx` — что он только инструмент сборки |
| З6 | Текст инвариантов в `CLAUDE.md` строже принятых решений: 2(а) — записи в перемещаемый профиль (`EdgeWebView`, `EdgeUpdate`, `UserAssist`, размер окна в `ComDlg32\CIDSizeMRU`) приняты решениями 61, 65, 56; 3 «дампов падений нет» — локальный дамп Crashpad живёт во временной папке до выхода (ADR 0007) | выровнять формулировки инвариантов с решениями (владелец) |
| З7 | `SetErrorMode` по документации касается необработанных исключений; отчёты WER о зависании окна (AppHang) создаются независимо от него | закрывается той же политикой WER из «Политик для службы ИТ»; в ручном аудите скрипт проверяет очередь WER |
| З8 | Recall (Copilot+ PC) сохраняет снимки экрана с текстом заметок и заголовком окна | **добавлено** в «Политики для службы ИТ» (`DisableAIDataAnalysis` или исключение `rhizomind.exe`) |
| З9 | В дереве Windows есть крейты `http` и `cookie` (через Tauri/wry) | только типы HTTP и разбор cookie, без ввода-вывода; разрешены `deny.toml` и `crates.json` |
| З10 | `network-audit.ps1` не запускался: в облачной среде нет Windows и PowerShell | первый прогон — QA-агент на компьютере владельца (с разрешения) или проверка разбора в CI на Windows: `[System.Management.Automation.Language.Parser]::ParseFile(...)`; ошибки скрипта не влияют на выводы — есть раздел «Без скрипта» |

## 4. Что требует решения владельца

1. **Контекстное меню браузерного движка (Р4).** (а) Выключить меню движка совсем — правый щелчок в тексте ничего не делает, копирование и вставка — клавишами; плюс: ни «Поделиться», ни «Печать»; минус: нет привычного меню с «Копировать»/«Вставить» и исправлениями орфографии. (б) Оставить только правку текста — отфильтровать пункты (Вырезать, Копировать, Вставить, Выделить всё, исправления орфографии), остальное убрать; плюс: привычно и без лишних каналов; минус: своя доработка через WebView2 API. (в) Оставить как есть и описать в `SECURITY.md`. **Рекомендация — (б)**, вместе с 2.17 (там же настройки движка WebView2).
2. **Запись `ConsentStore` в перемещаемом профиле (Р5).** Если ручной аудит подтвердит, что её пишет Windows при запросе движка и в ней нет данных хранилища: (а) принять, как решения 61 и 65; (б) искать способ, чтобы движок не обращался к диспетчеру возможностей. **Рекомендация — (а)** после подтверждения аудитом.
3. **Ручной аудит (Б1)** — назначить прогон на второй машине до задачи 3.4.

## 5. Ручной аудит

Заполняется по `results.md` владельца: дата, версия WebView2, итог по сети процессов Rhizomind и движка, обращения служб Windows против раздела «Ожидаемые обращения к Microsoft», записи вне хранилища, итог по каждому флагу (подтверждён / не опровергнут / опровергнут), пункты контекстного меню.

_Не проведён._
