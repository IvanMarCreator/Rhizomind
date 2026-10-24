# Rhizomind

**Rhizomind** — локальная база знаний со связанными заметками. Пользовательский опыт как в Obsidian, но это самостоятельный продукт.

Название: *rhizome* (ризома — сеть без центра и иерархии, где любой узел связан с любым) + *mind*.

## Принципы

- **Только локально.** Никакой синхронизации, облака и телеметрии. Все данные остаются на вашем диске.
- **Обычные Markdown-файлы.** Хранилище — это просто папка с `.md`-файлами, которые можно открыть в любом редакторе.
- **Связи между знаниями.** Вики-ссылки `[[Заметка]]`, обратные ссылки и граф связей.

## Синтаксис

- `[[Заметка]]`, `[[Заметка|псевдоним]]`, `[[Заметка#Заголовок]]` — внутренние ссылки
- `![[файл]]` — встраивание заметки или файла
- `#тег`, составные через подчёркивание: `#Рабочие_заметки`
- блок свойств `---` в начале заметки (в 1.0 из него читаются теги)

## Статус

Идёт работа над выпуском 0.1 Secure-редакции: требования и план выпусков утверждены, архитектура принята. Стек: **Tauri 2** — ядро на Rust, интерфейс на Svelte 5 + TypeScript с редактором CodeMirror 6 в системном WebView (на Windows — WebView2). Код в [`prototype/`](prototype/README.md) — ранний прототип на Electron, справочник, а не основа.

- [Требования и план выпусков](docs/product/requirements.md)
- [Архитектура](docs/ARCHITECTURE.md)
- [Модель безопасности для ИБ](docs/SECURITY.md)
- [Правила работы агентов](CLAUDE.md)

## Разработка

Нужно:
- **Rust** через [rustup](https://rustup.rs/). Версия закреплена в `rust-toolchain.toml` (сейчас 1.99.1) и ставится сама при первом вызове `cargo`.
- **Node.js 22** (не ниже 22.19).
- Windows: WebView2 Runtime (есть в Windows 11 и почти во всех Windows 10) и Visual Studio Build Tools с компонентом «Разработка классических приложений на C++».
- Linux (Ubuntu 24.04): `sudo apt-get install libwebkit2gtk-4.1-dev librsvg2-dev`; для сценарных тестов ещё `webkit2gtk-driver xvfb`.
- Для `npm run verify`: `cargo install --locked cargo-deny@0.20.2`. Для сценарных тестов: `cargo install --locked tauri-driver@2.1.0`, на Windows — msedgedriver той же версии, что WebView2 (путь — в переменной `MSEDGEDRIVER`).

```
npm ci                    # зависимости интерфейса строго по package-lock.json
npm run dev               # запуск в режиме разработки (tauri dev, Vite на localhost:5173)
npm run verify            # все проверки: типы, линтер, Vitest, сеть в интерфейсе, лицензии npm,
                          # cargo fmt, clippy, cargo test, cargo deny, cargo tree по целям
npm run build             # выпускной исполняемый файл: src-tauri/target/release/rhizomind(.exe)
npm run build:e2e         # сборка для сценарного теста (функция webdriver: порт отладки для msedgedriver)
npm run test:e2e          # сценарный тест на этой сборке;
                          # на Linux без экрана: xvfb-run -a npm run test:e2e
npm run package:portable  # Windows: release/rhizomind-<версия>-windows-x64-portable.zip
npm run package:msi       # Windows: установщик MSI (для всех пользователей, нужны права администратора)
npm run package:nsis      # Windows: установщик NSIS для текущего пользователя
npm run sbom              # перечень зависимостей (CycloneDX) в release/; нужен cargo-cyclonedx 0.5.9
```

Сборки не подписаны. Установщики не скачивают WebView2 (`webviewInstallMode: skip`); сам бандлер Tauri при сборке MSI и NSIS скачивает инструменты WiX и NSIS — это происходит только на машине сборки.

Проверки для ИБ лежат в `tools/` (у каждой свои тесты), конфигурация `cargo-deny` — в `src-tauri/deny.toml`, зависимости и их лицензии — в [docs/dependencies.md](docs/dependencies.md).

## Лицензия

Rhizomind распространяется под лицензией [GNU GPL v3.0](LICENSE).

Это значит, что программу можно свободно использовать, изучать, изменять и распространять, но любые производные работы, включая плагины, встроенные в программу, должны распространяться под той же лицензией с открытым исходным кодом.
