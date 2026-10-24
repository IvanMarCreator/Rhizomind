# Изменения

Формат: [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/), версии — `0.МИНОР.ПАТЧ` до 1.0.

## [Не выпущено]

### Добавлено
- Требования и план выпусков Secure-редакции 1.0.
- Архитектурные решения ADR 0001–0007.
- Спецификация выпуска 0.1 «Основа».
- Проверка в GitHub Actions (CI) на Windows и Linux, Node.js 22.
- `.gitattributes`: исходники с LF, тестовые образцы без преобразования концов строк.
- Каркас приложения: Electron 45, TypeScript (strict), Svelte 5, Vite 8; окно с заголовком «Rhizomind» грузит интерфейс через протокол `app://rhizomind/` (изоляция контекста, песочница, без Node.js в интерфейсе, CSP).
- Скрипты `dev`, `build`, `typecheck`, `lint`, `test`, `test:e2e`, `verify`, `package:portable`, `package:installer`, `sbom`.
- Проверка исходников на сетевой код (`check:network`) и лицензий production-зависимостей на совместимость с GPL-3.0 (`check:licenses`).
- Сценарный тест «окно открылось» (Playwright + Electron) со снимком экрана.
- Сборка electron-builder: asar, Electron Fuses, `publish: null`, портативный zip и установщик NSIS для Windows.
- CI: проверки, сборка и сценарные тесты на Windows и Linux; портативная сборка Windows и снимки экрана загружаются как артефакты.
- Перечень зависимостей с лицензиями: `docs/dependencies.md`.

### Изменено
- `docs/SECURITY.md`: ссылки на реализованные меры изоляции, CSP, проверку сети и Fuses ведут на новый код.
- Прототип перенесён в `prototype/` как справочник; ссылки в документации обновлены.
