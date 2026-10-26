# Один экземпляр программы: плагин Tauri или свой механизм (26.10.2026)

Задача 0.5, решение владельца 57: повторный запуск не открывает второй процесс, а переключает на уже открытое окно. Вопрос — можно ли взять `tauri-plugin-single-instance`, не нарушив инвариант 1 («в коде приложения и зависимостях нет сетевых возможностей вообще»), или нужен свой механизм.

## Итог

**Свой механизм** (`src-tauri/src/single_instance/`). Плагин не проходит `cargo deny` ни на одной из 5 десктопных целей: он сам включает функцию `net` у `tokio` (запрещена в `src-tauri/deny.toml`), а на Linux тянет `zbus` 5 с `async-io` и `socket2` (TCP-транспорт D-Bus). Плюс плагин проверяет экземпляр в `setup`, то есть **после** того, как ядро уже создало временную папку WebView2, — второй процесс успевал бы писать на диск.

## Плагин `tauri-plugin-single-instance`

Первоисточник — исходник крейта с crates.io (`tauri-plugin-single-instance-2.5.4.crate`, файлы `Cargo.toml`, `src/platform_impl/{windows,linux,macos}.rs`).

- **Версия под Tauri 2.12.5:** 2.5.4 — последняя ветки 2 (требует `tauri = "^2.12"`); 3.0.0-alpha.3 — под Tauri 3.
- **Зависимости (все цели):** `tokio = { version = "1", features = ["net"] }`, `serde`, `serde_json`, `thiserror`, `tracing`, необязательные `semver` и `tauri-plugin-deep-link`. Функция `net` нужна только macOS-реализации (`tokio::net::UnixListener`), но включена безусловно.
- **Windows** (`windows-sys`): именованный мьютекс `<identifier>-sim` (без префикса — пространство имён сессии), скрытое окно класса `<identifier>-sic`; второй экземпляр ищет его `FindWindowW`, отдаёт право на передний план `AllowSetForegroundWindow` и шлёт `WM_COPYDATA` с рабочей папкой и аргументами, затем `exit(0)`. На диск и в реестр не пишет. Сети нет, но проверка — в `setup` плагина.
- **Linux** (`zbus = "5.9"`, функции по умолчанию): имя на сессионной шине D-Bus `org.<identifier>.SingleInstance`, второй экземпляр вызывает метод первого. `zbus` 5.19 по умолчанию — `async-io` (+ `async-executor`, `async-fs`, `async-process`, `blocking`), адреса шины поддерживают транспорт `tcp:` (`async-io::Async<TcpStream>`), в дереве — `socket2` и `rustix` с функцией `net`. На диск не пишет.
- **macOS:** Unix-сокет `/tmp/<identifier>_si.sock` (файл в `/tmp`, удаляется при выходе), `tokio::net::UnixListener`.

### Проверка на нашем дереве

Копия рабочей копии, в `src-tauri/Cargo.toml` добавлено `tauri-plugin-single-instance = "=2.5.4"`:

```
$ cargo tree -e normal,no-proc-macro --target <цель> --prefix none | grep -E '^(zbus|async-io|socket2|tokio|mio|rustix) '
x86_64-pc-windows-msvc:   mio v1.2.4, socket2 v0.6.5, tokio v1.54.1 (feature "net")
x86_64-apple-darwin:      mio v1.2.4, rustix v1.1.5, socket2 v0.6.5, tokio v1.54.1 (feature "net")
x86_64-unknown-linux-gnu: async-io v2.6.0, mio v1.2.4, rustix v1.1.5, socket2 v0.6.5, tokio v1.54.1 (feature "net"), zbus v5.19.0

$ cargo deny --manifest-path src-tauri/Cargo.toml check bans
error[banned]: crate 'socket2 = 0.6.5' is explicitly banned
error[feature-banned]: feature 'net' for crate 'mio = 1.2.4' is explicitly denied
error[feature-banned]: feature 'net' for crate 'tokio = 1.54.1' is explicitly denied
bans FAILED
```

`check-cargo-tree` упал бы так же (запрещённые крейты из `deny.toml` + новые крейты в наборе). Обойти нельзя: функцию `net` включает сам плагин, отключить её из нашего `Cargo.toml` Cargo не позволяет.

## Свой механизм

Без новых крейтов: только `windows` (уже в дереве; добавлены функции привязок `Win32_Security`, `Win32_Graphics_Gdi`, `Win32_System_DataExchange`) и `libc` (уже в дереве на unix).

- **Windows.** Именованный мьютекс `Local\Rhizomind.SingleInstance` — объект ядра в пространстве имён сессии пользователя, не файл: на диск и в реестр ничего не пишется, ОС удаляет его, когда закрыт последний дескриптор (в том числе при падении процесса). Первый экземпляр создаёт скрытое окно «только для сообщений» (`HWND_MESSAGE`, класс `Rhizomind.SingleInstance`) — его не видно, его нет в панели задач и в `EnumWindows`. Второй экземпляр находит его `FindWindowExW(HWND_MESSAGE, …)`, отдаёт первому право на передний план (`AllowSetForegroundWindow`) и шлёт `WM_COPYDATA` с меткой протокола (полезной нагрузки сейчас нет; путь хранилища из аргументов — потом тем же сообщением). Первый выводит последнее активное окно вперёд. Пока первый экземпляр запускается или завершается (мьютекс есть, окна ещё/уже нет), второй повторяет попытку до 5 с, затем молча выходит. Работает в пределах одного сеанса пользователя; другой пользователь на том же компьютере (другой сеанс) запускает свой экземпляр.
- **Linux, macOS.** Файл-замок `dirs::data_local_dir()/Rhizomind/instance.lock` (рядом с `vaults.json`; Linux `~/.local/share/Rhizomind/`, macOS `~/Library/Application Support/Rhizomind/`), `flock(LOCK_EX | LOCK_NB)`. Файл пустой, создаётся первым запуском и не удаляется (удаление файла-замка даёт гонки); замок снимает ОС при выходе или падении процесса. Связи с первым экземпляром нет: второй пишет «Rhizomind уже запущен» в stderr и выходит. Дескриптор открыт с `O_CLOEXEC` (по умолчанию в Rust) — дочерние процессы (`xdg-open` для внешних ссылок) замок не наследуют.
- **Порядок.** Проверка — первой строкой `run()`, до `Shell::prepare()` (временная папка WebView2, уборка остатков), до чтения `vaults.json` и до создания движка: второй процесс не пишет ничего. Мьютекс и замок освобождаются при `RunEvent::Exit` до ожидания процесса браузера — новый запуск сразу после закрытия окна не попадает на «умирающий» экземпляр.
- **Имена** постоянные: без путей, имён хранилищ и пользователя. Тестовая сборка (`webdriver`) — с суффиксом `.webdriver` (Windows) и файлом `instance-webdriver.lock` в папке профиля сценарного теста, чтобы не мешать выпускной сборке на той же машине.

### Что не входит

- Передача пути хранилища из аргументов (аргументов пока нет) — формат сообщения уже несёт метку протокола.
- Активация первого экземпляра на Linux/macOS (нужен канал связи: D-Bus или Unix-сокет — оба вне инварианта без отдельного решения).
