# Исследование вариантов архитектуры (24.10.2026)

Сводка трёх параллельных исследований для решений из `docs/ARCHITECTURE.md`. Версии получены через страницы npm и GitHub; часть дат не подтверждена, а прямой доступ к npm registry у исследователей был закрыт. Метки: **факт** — из первоисточника, **сторонний** — из issues, **оценка** — инженерная прикидка без замеров. Перед фиксацией в спецификации версии перепроверяются при установке зависимостей.

## Редактор

| Кандидат | Версия | Лицензия | Вывод |
|---|---|---|---|
| CodeMirror 6 | view 6.43, state 6.7, @lezer/markdown 1.7 | MIT | Документ остаётся текстом, разметку прячут decorations: запись байт в байт по устройству. Live preview пишется самим (так сделан Obsidian). |
| Milkdown (ProseMirror) | 7.22 | MIT | Пишет файл заново сериализатором remark: переформатирует Markdown пользователя. |
| Tiptap (ProseMirror) | 3.3x | MIT, Pro-часть закрытая | Те же потери при записи; Pro-расширения из приватного реестра. |
| Lexical | 0.52 | MIT | Версия 0.x, запись Markdown не документирована. |

Готовые надстройки live preview на CM6 (`codemirror-markdown-hybrid`, Atomic Editor) — только как справочник: лишние тяжёлые зависимости или привязка к React.

## Фреймворк интерфейса

Svelte 5.57, SolidJS 1.9, React 19.3, Preact 11, Lit 3.3 — все MIT (Lit BSD-3). Vite 8.3 (Rolldown), Vitest 5, Playwright 1.64 (Electron — экспериментально). Редактор, граф и раскладка — императивные виджеты без фреймворка, поэтому фреймворк отвечает только за «обвязку»: дерево файлов, окна, настройки.

## Раскладка панелей

| Кандидат | Версия | Лицензия | Вывод |
|---|---|---|---|
| dockview-core | 8.6 (19.10.2026) | MIT | Без фреймворка и зависимостей, сериализация и перетаскивание есть; клавиатурная доступность не подтверждена; частые мажорные версии. |
| flexlayout-react | 0.11 | MIT | Лучшая подтверждённая доступность, но только React. |
| Golden Layout 2 | 2.6 (2022) | MIT | Заброшен. |
| Lumino | 2.10 | BSD-3 | Зрелый, но 10 пакетов и свой стиль. |

Формат раскладки в хранилище должен быть своим, иначе хранилища привязываются к внутреннему формату чужой библиотеки.

## Граф

| Кандидат | Версия | Лицензия | Вывод |
|---|---|---|---|
| sigma.js + graphology | 4.0.3 (v4 вышла 08.10.2026) | MIT | WebGL, раскладка ForceAtlas2 в Web Worker или на GPU. Кириллица в подписях v4 не проверена. |
| cosmos.gl | 3.5 | MIT | Физика на GPU, но нет подписей и фильтров. |
| Pixi.js + d3-force | 8.22 / 3.0 | MIT / ISC | Путь Obsidian; всё писать самим; у Obsidian потолок ~25–30 тыс. заметок (сторонний). |
| Cytoscape.js | 3.34 | MIT | Canvas на CPU, для 50 тыс. сомнительно. |
| Cosmograph | 2.5 | CC-BY-NC | **Не подходит:** лицензия несовместима с GPL, в зависимостях сетевой клиент. |

Независимых замеров на 50 тыс. узлов нет. Перед выбором — прототип-замер.

## Индекс

| Кандидат | Вывод |
|---|---|
| JSON-снимок + поиск в памяти (MiniSearch) | Без нативного кода. Метаданные ~30 МБ, полнотекстовый индекс в памяти 150–500 МБ на 50 тыс. заметок (оценка). |
| better-sqlite3 13 + FTS5 | Индекс на диске, память не растёт. Нативный модуль (Node-API, пребилды в пакете): распаковка из asar, подпись. Русская морфология — стеммер на JS перед записью. |
| node:sqlite (Node 24 в Electron 45) | Без нативных зависимостей; работа в main-процессе Electron не подтверждена. |
| sql.js / wa-sqlite | Не подходят: нет FTS5 или запись в профиль Chromium. |

## Слежение за папкой

| Кандидат | Вывод |
|---|---|
| @parcel/watcher 2.6 | Нативный, буфер ~1 МБ, сообщает о переполнении, исключение `.rhizomind` без событий. Переименование = удаление + создание. |
| chokidar 5 | Чистый JS поверх fs.watch, начальный обход всех файлов. |
| fs.watch recursive | Буфер 4 КБ, переполнение не обрабатывается: риск потери событий. |

В любом варианте нужна сверка по дате и размеру при старте и полный пересмотр после переполнения.

## Electron и следы на диске

- Стабильная версия: Electron 45.0 (21.10.2026, Chromium 156, Node 24.22), поддержка до 27.04.2027; мажор каждые 8 недель, поддерживаются три последних (факт). electron-builder 26.16.
- **«Ровно один файл в профиле ОС» средствами Electron недостижим** (факт по исходникам): Chromium всегда создаёт служебную папку сессии (`Local State` и др.), а сессия по умолчанию — персистентная. Достижимо: все окна в партиции без `persist:`, служебная папка Chromium (`sessionData`) перенаправлена во временную папку и удаляется при выходе; кэши шейдеров и HTTP отключены флагами; `crashReporter.start()` не вызывается, тогда Crashpad не запускается. Проверка — Process Monitor на чистой установке.
- **Сетевые находки, которых нет в прототипе:**
  - словари проверки орфографии скачиваются с серверов Google даже при выключенной проверке; помогает `setSpellCheckerLanguages([])` в `session-created` (сторонний, issue electron#22995);
  - Chromium ищет прокси через WPAD (`http://wpad/wpad.dat` в локальной сети); лечится `--no-proxy-server` (сторонний);
  - страховка: отмена запросов на каждой сессии, включая сессию по умолчанию, и `--host-resolver-rules="MAP * ~NOTFOUND"`; сетевой аудит перед выпуском.

## Источники (основные)

- https://www.electronjs.org/blog/electron-45-0, https://releases.electronjs.org/schedule
- https://www.electronjs.org/docs/latest/api/app, https://www.electronjs.org/docs/latest/api/session, https://www.electronjs.org/docs/latest/api/crash-reporter
- https://github.com/electron/electron/issues/22995
- https://docs.obsidian.md/Plugins/Editor/Decorations, https://codemirror.net/
- https://github.com/Milkdown/milkdown, https://tiptap.dev/docs/guides/pro-extensions, https://lexical.dev/docs/packages/lexical-markdown
- https://www.sigmajs.org/, https://github.com/cosmosgl/graph, https://forum.obsidian.md/t/obsidian-graph-view-doesnt-work-for-a-large-vault/106287
- https://github.com/dockview/dockview, https://github.com/caplin/FlexLayout, https://github.com/golden-layout/golden-layout
- https://github.com/WiseLibs/better-sqlite3/releases, https://nodejs.org/api/sqlite.html, https://sqlite.org/fts5.html, https://github.com/lucaong/minisearch
- https://github.com/parcel-bundler/watcher, https://github.com/paulmillr/chokidar
