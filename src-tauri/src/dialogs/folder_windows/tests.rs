//! Окно выбора папки на Windows: опции окна (обычный прогон) и проверка
//! следов на настоящем окне (`--ignored`, отдельный шаг CI на Windows).

use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    BM_CLICK, EnumChildWindows, FindWindowW, GetClassNameW, GetDlgItem, GetWindowTextW, IDCANCEL,
    IDOK, IsWindow, PostMessageW, SendMessageW, SetForegroundWindow, WM_COMMAND,
};
use windows::core::BOOL;
use windows::core::{HSTRING, PCWSTR};
use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW,
};

use super::*;

#[test]
fn private_settings_keep_the_folder_out_of_recent_lists() {
    let options = PRIVATE.options.0;
    for (flag, name) in [
        (FOS_PICKFOLDERS, "FOS_PICKFOLDERS"),
        (FOS_DONTADDTORECENT, "FOS_DONTADDTORECENT"),
        (FOS_FORCEFILESYSTEM, "FOS_FORCEFILESYSTEM"),
        (FOS_PATHMUSTEXIST, "FOS_PATHMUSTEXIST"),
    ] {
        assert_eq!(options & flag.0, flag.0, "нет опции {name}");
    }
    assert_ne!(CLIENT_GUID, GUID::zeroed());
    assert_eq!(
        (PRIVATE.client_guid, PRIVATE.clear_client_data),
        (Some(CLIENT_GUID), true),
        "постоянный GUID клиента и ClearClientData после закрытия"
    );
}

/// Уникальная метка (имя папки и заголовок окна).
fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("{prefix}-{}-{nanos}", std::process::id())
}

/// «Пользователь» в отдельном потоке: ждёт окно с заголовком `title`, в поле
/// имени которого уже стоит путь к папке `folder` с `\` в конце; первой
/// кнопкой переходит в папку, второй — выбирает её.
///
/// Каждый шаг ждёт видимого признака в окне, а не паузы (задача 0.10): окно
/// готово — есть адресная строка; переход сделан — в адресной строке имя
/// `folder`. Прежние паузы 2 с и 3 с иногда не дожидались перехода, и
/// вторая кнопка выбирала папку по умолчанию («Документы»).
fn press_select(title: String, folder: String) -> std::thread::JoinHandle<Vec<String>> {
    std::thread::spawn(move || {
        let mut log = Vec::new();
        let Some(window) = find_dialog(&title, 200) else {
            log.push("окно не найдено".to_owned());
            return log;
        };
        log.push("окно найдено".to_owned());
        // Папка по умолчанию — «Документы»: в адресе имя, а не путь.
        let ready = wait_for(Duration::from_secs(20), || address(window));
        log.push(format!("адрес до перехода: {ready:?}"));
        // SAFETY: сообщения окну диалога, которое живёт в другом потоке.
        unsafe {
            let _ = SetForegroundWindow(window);
            let button = GetDlgItem(Some(window), IDOK.0).ok();
            let _ = PostMessageW(
                Some(window),
                WM_COMMAND,
                WPARAM(IDOK.0 as usize),
                LPARAM(button.map_or(0, |b| b.0 as isize)),
            );
            log.push(format!("WM_COMMAND {}", IDOK.0));
            let moved = wait_for(Duration::from_secs(20), || {
                if !IsWindow(Some(window)).as_bool() {
                    return Some("окно закрылось".to_owned());
                }
                address(window).filter(|text| text.contains(&folder))
            });
            log.push(format!("после перехода: {moved:?}"));
            if IsWindow(Some(window)).as_bool() {
                if let Some(button) = button {
                    SendMessageW(button, BM_CLICK, None, None);
                    log.push("BM_CLICK".to_owned());
                }
                let closed = wait_for(Duration::from_secs(20), || {
                    (!IsWindow(Some(window)).as_bool()).then_some(())
                });
                if closed.is_none() {
                    let _ = PostMessageW(
                        Some(window),
                        WM_COMMAND,
                        WPARAM(IDCANCEL.0 as usize),
                        LPARAM(0),
                    );
                    log.push("окно не закрылось: IDCANCEL".to_owned());
                }
            }
        }
        log
    })
}

/// Окно диалога (класс `#32770`) с заголовком `title`: до `tries` × 100 мс.
fn find_dialog(title: &str, tries: u32) -> Option<HWND> {
    let title = HSTRING::from(title);
    let class = HSTRING::from("#32770");
    for _ in 0..tries {
        std::thread::sleep(Duration::from_millis(100));
        // SAFETY: только поиск окна по классу и заголовку.
        if let Ok(found) = unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR(title.as_ptr())) } {
            return Some(found);
        }
    }
    None
}

/// Ждёт, пока `probe` вернёт значение, проверяя каждые 100 мс; не дольше `limit`.
fn wait_for<T>(limit: Duration, mut probe: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = std::time::Instant::now() + limit;
    loop {
        if let Some(value) = probe() {
            return Some(value);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Текст адресной строки окна выбора (панель `ToolbarWindow32` вида
/// «Address: Documents», «Address: C:\…»), если она уже есть.
fn address(window: HWND) -> Option<String> {
    unsafe extern "system" fn visit(child: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` — указатель на `Vec<String>` вызывающего, живой на
        // время перебора; окна — того же процесса.
        unsafe {
            let mut class = [0u16; 64];
            let len = GetClassNameW(child, &mut class);
            if String::from_utf16_lossy(&class[..len.max(0) as usize]) == "ToolbarWindow32" {
                let mut text = [0u16; 1024];
                let len = GetWindowTextW(child, &mut text);
                let text = String::from_utf16_lossy(&text[..len.max(0) as usize]);
                if text.contains(':') {
                    (*(found.0 as *mut Vec<String>)).push(text);
                }
            }
        }
        BOOL(1)
    }
    let mut found: Vec<String> = Vec::new();
    // SAFETY: перебор дочерних окон; указатель на `found` живёт до возврата.
    unsafe {
        let _ = EnumChildWindows(
            Some(window),
            Some(visit),
            LPARAM(&mut found as *mut Vec<String> as isize),
        );
    }
    found.into_iter().next()
}

/// Ждёт окно с заголовком `title` до `tries` × 100 мс и нажимает кнопку `id`.
fn press(title: String, id: i32, tries: u32) -> std::thread::JoinHandle<Vec<String>> {
    std::thread::spawn(move || {
        let mut log = Vec::new();
        let title = HSTRING::from(title);
        let class = HSTRING::from("#32770");
        let mut window = None;
        for _ in 0..tries {
            std::thread::sleep(Duration::from_millis(100));
            // SAFETY: только поиск окна по классу и заголовку.
            if let Ok(found) =
                unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR(title.as_ptr())) }
            {
                window = Some(found);
                break;
            }
        }
        let Some(window) = window else {
            log.push("окно не найдено".to_owned());
            return log;
        };
        log.push("окно найдено".to_owned());
        std::thread::sleep(Duration::from_secs(2)); // окно загружает папку
        // SAFETY: сообщения окну диалога, которое живёт в другом потоке.
        unsafe {
            let _ = SetForegroundWindow(window);
            let button = GetDlgItem(Some(window), id).ok();
            let _ = PostMessageW(
                Some(window),
                WM_COMMAND,
                WPARAM(id as usize),
                LPARAM(button.map_or(0, |b| b.0 as isize)),
            );
            log.push(format!("WM_COMMAND {id}"));
            std::thread::sleep(Duration::from_secs(3));
            if IsWindow(Some(window)).as_bool() {
                if let Some(button) = button {
                    SendMessageW(button, BM_CLICK, None, None);
                    log.push("BM_CLICK".to_owned());
                }
                std::thread::sleep(Duration::from_secs(3));
            }
            if IsWindow(Some(window)).as_bool() {
                let _ = PostMessageW(
                    Some(window),
                    WM_COMMAND,
                    WPARAM(IDCANCEL.0 as usize),
                    LPARAM(0),
                );
                log.push("окно не закрылось: IDCANCEL".to_owned());
            }
        }
        log
    })
}

/// Строка в байтах, как она лежит в значениях реестра и файлах: UTF-16LE и ANSI.
fn needles(text: &str) -> Vec<Vec<u8>> {
    let wide = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    vec![wide, text.as_bytes().to_vec()]
}

fn contains(haystack: &[u8], needles: &[Vec<u8>]) -> bool {
    needles
        .iter()
        .any(|n| !n.is_empty() && haystack.windows(n.len()).any(|w| w == n.as_slice()))
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Ключи и значения под `HKCU\<path>`, где встречается `needles` (в имени
/// ключа, имени значения или данных). Подключи из `skip` не обходятся.
fn registry_hits(path: &str, skip: &[&str], needles: &[Vec<u8>]) -> Vec<String> {
    let mut hits = Vec::new();
    walk_registry(path, skip, needles, &mut hits);
    hits
}

fn walk_registry(path: &str, skip: &[&str], needles: &[Vec<u8>], hits: &mut Vec<String>) {
    if skip.iter().any(|s| s.eq_ignore_ascii_case(path)) {
        return;
    }
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: чтение реестра текущего пользователя; ключ закрывается ниже.
    if unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(path).as_ptr(),
            0,
            KEY_READ,
            &mut key,
        )
    } != ERROR_SUCCESS
    {
        return;
    }
    let mut name = vec![0u16; 32_768];
    let mut data = vec![0u8; 1 << 16];
    let mut index = 0;
    loop {
        let mut name_len = name.len() as u32;
        let mut data_len = data.len() as u32;
        let mut kind = 0;
        // SAFETY: буферы с указанными длинами.
        let rc = unsafe {
            RegEnumValueW(
                key,
                index,
                name.as_mut_ptr(),
                &mut name_len,
                std::ptr::null(),
                &mut kind,
                data.as_mut_ptr(),
                &mut data_len,
            )
        };
        if rc == ERROR_NO_MORE_ITEMS {
            break;
        }
        if rc == ERROR_MORE_DATA {
            data.resize(data_len as usize + 1024, 0);
            continue;
        }
        if rc == ERROR_SUCCESS {
            let value_name = String::from_utf16_lossy(&name[..name_len as usize]);
            let name_bytes: Vec<u8> = name[..name_len as usize]
                .iter()
                .flat_map(|c| c.to_le_bytes())
                .collect();
            if contains(&name_bytes, needles) || contains(&data[..data_len as usize], needles) {
                hits.push(format!("HKCU\\{path} : {value_name}"));
            }
        }
        index += 1;
    }
    let mut subkeys = Vec::new();
    let mut index = 0;
    loop {
        let mut name_len = name.len() as u32;
        // SAFETY: буфер с указанной длиной.
        let rc = unsafe {
            RegEnumKeyExW(
                key,
                index,
                name.as_mut_ptr(),
                &mut name_len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if rc != ERROR_SUCCESS {
            break;
        }
        subkeys.push(String::from_utf16_lossy(&name[..name_len as usize]));
        index += 1;
    }
    // SAFETY: ключ открыт выше.
    unsafe { RegCloseKey(key) };
    for sub in subkeys {
        let child = if path.is_empty() {
            sub.clone()
        } else {
            format!("{path}\\{sub}")
        };
        if contains(sub.as_bytes(), needles) {
            hits.push(format!("HKCU\\{child}"));
        }
        walk_registry(&child, skip, needles, hits);
    }
}

/// Файлы под `root`, в имени которых есть `name`, и файлы под `contents`
/// (часть `root`), в содержимом которых есть `needles`.
fn file_hits(root: &Path, name: &str, contents: &Path, needles: &[Vec<u8>]) -> Vec<String> {
    const REPARSE_POINT: u32 = 0x400;
    let mut hits = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if entry.file_name().to_string_lossy().contains(name) {
                hits.push(path.display().to_string());
            }
            if meta.file_attributes() & REPARSE_POINT != 0 {
                continue;
            }
            if meta.is_dir() {
                stack.push(path);
            } else if path.starts_with(contents)
                && std::fs::read(&path).is_ok_and(|bytes| contains(&bytes, needles))
            {
                hits.push(format!("{} (содержимое)", path.display()));
            }
        }
    }
    hits
}

/// Где путь к папке `name` виден в перемещаемом профиле: весь HKCU, кроме
/// `Software\Classes\Local Settings` (не перемещается), и `%APPDATA%` (имена
/// файлов; содержимое — в недавних документах).
fn roaming_hits(name: &str) -> Vec<String> {
    let needles = needles(name);
    let appdata = PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA"));
    let recent = appdata.join(r"Microsoft\Windows\Recent");
    let mut hits = registry_hits("", &[r"Software\Classes\Local Settings"], &needles);
    hits.extend(file_hits(&appdata, name, &recent, &needles));
    hits
}

/// Настройки окна по имени прогона.
fn settings_for(kind: &str) -> Settings {
    match kind {
        "private" => PRIVATE,
        // Наши опции без ClearClientData: запись LastVisitedPidlMRU остаётся.
        "no-clear" => Settings {
            clear_client_data: false,
            ..PRIVATE
        },
        // Как rfd 0.16.1 (было до задачи 0.8).
        "rfd" => Settings {
            options: FOS_PICKFOLDERS,
            client_guid: None,
            clear_client_data: false,
        },
        other => panic!("неизвестный прогон {other}"),
    }
}

const CHILD_KIND: &str = "RHIZOMIND_T08_CHILD";
const CHILD_FOLDER: &str = "RHIZOMIND_T08_FOLDER";

/// Окно в отдельном процессе (как приложение, которое потом закрыли):
/// часть записей оболочка делает только при выходе процесса. Запускается
/// из [`pick_temp_folder`]; без переменных окружения ничего не делает.
#[test]
#[ignore = "показывает окно ОС; запускается из chosen_folder_path_does_not_reach_roaming_profile"]
fn dialog_in_child_process() {
    let (Some(kind), Some(folder)) = (std::env::var_os(CHILD_KIND), std::env::var_os(CHILD_FOLDER))
    else {
        return;
    };
    let kind = kind.to_string_lossy().into_owned();
    let folder = PathBuf::from(folder);
    let title = unique("Rhizomind проверка окна");
    let folder_name = folder
        .file_name()
        .expect("имя папки")
        .to_string_lossy()
        .into_owned();
    let user = press_select(title.clone(), folder_name);
    let target = folder.clone();
    // Как пользователь: путь с `\` в конце в поле имени — первая кнопка
    // переходит в папку, вторая выбирает её. `SetFolder` не вызывается.
    let chosen = show(None, &title, settings_for(&kind), move |dialog| {
        let mut text = target.into_os_string();
        text.push("\\");
        // SAFETY: вызов из потока окна с инициализированным COM.
        unsafe { dialog.SetFileName(&HSTRING::from(text)) }
    });
    let log = user.join().expect("поток пользователя");
    eprintln!("[{kind}] окно: {log:?}; ответ: {chosen:?}");
    let expected = std::fs::canonicalize(&folder).expect("канонический путь");
    let chosen = chosen.map(|p| std::fs::canonicalize(p).expect("канонический ответ"));
    assert_eq!(
        chosen.as_deref(),
        Some(expected.as_path()),
        "окно вернуло выбранную папку"
    );
}

/// Создаёт временную папку `<метка>-parent\<метка>-vault`, выбирает её в окне
/// с настройками `kind` в отдельном процессе и возвращает метку, которая есть
/// в имени и самой папки, и её родителя.
fn pick_temp_folder(kind: &str) -> (tempfile::TempDir, String) {
    let name = unique(&format!("rhz-t08-{kind}"));
    let parent = tempfile::Builder::new()
        .prefix(&format!("{name}-parent"))
        .rand_bytes(0)
        .tempdir()
        .expect("временная папка");
    let folder = parent.path().join(format!("{name}-vault"));
    std::fs::create_dir(&folder).expect("папка хранилища");
    let status = std::process::Command::new(std::env::current_exe().expect("файл теста"))
        .args([
            "--ignored",
            "--exact",
            "dialogs::folder_windows::tests::dialog_in_child_process",
            "--nocapture",
        ])
        .env(CHILD_KIND, kind)
        .env(CHILD_FOLDER, &folder)
        .status()
        .expect("процесс окна");
    assert!(status.success(), "процесс окна {kind}: {status}");
    // Недавние документы пишет Explorer.EXE по вызову окна — не сразу.
    std::thread::sleep(Duration::from_secs(5));
    (parent, name)
}

const LAST_VISITED: &str = r"ComDlg32\LastVisitedPidlMRU";

/// Настоящее окно на настоящем рабочем столе: в обычный прогон не входит.
/// CI на Windows: `cargo test -p rhizomind --lib dialogs::folder_windows -- --ignored`.
/// Каждое окно — в своём процессе, поиск — после его выхода.
/// С `RHIZOMIND_T08_CONTROL=1` — ещё и контроль поиска: окно с опциями rfd
/// (как было) пишет путь в перемещаемый профиль, и поиск его находит; окно с
/// нашими опциями без `ClearClientData` — в журнал (на рабочей машине не включать).
///
/// Ограничение: в этой среде (тестовый процесс Rust на раннере) Windows не
/// пишет `ComDlg32\LastVisitedPidlMRU` вообще — даже с опциями rfd, при
/// переходе в папку в самом окне. Действие `ClearClientData` на эту запись
/// подтверждено только опытом 0.6 (зонд на C#, Process Monitor), этим тестом
/// не покрыто (`docs/SECURITY.md`).
#[test]
#[ignore = "показывает окно ОС; отдельный шаг CI на Windows"]
fn chosen_folder_path_does_not_reach_roaming_profile() {
    let (_folder, name) = pick_temp_folder("private");
    let roaming = roaming_hits(&name);
    eprintln!("Roaming (всё HKCU, кроме Classes\\Local Settings; %APPDATA%): {roaming:#?}");
    let local = registry_hits(
        r"Software\Classes\Local Settings\Software\Microsoft\Windows\Shell",
        &[],
        &needles(&name),
    );
    // ShellBags (Local, не перемещаются) по расследованию 0.6 хранят путь при
    // любых опциях — только в журнал: допустимая локальная запись (инвариант 2).
    eprintln!("Local (Classes\\Local Settings, ShellBags): {local:#?}");

    let mut failures = Vec::new();
    if !roaming.is_empty() {
        failures.push(format!("наше окно: путь в Roaming: {roaming:#?}"));
    }
    if std::env::var_os("RHIZOMIND_T08_CONTROL").is_some_and(|v| v == "1") {
        let (_no_clear, no_clear_name) = pick_temp_folder("no-clear");
        let no_clear = roaming_hits(&no_clear_name);
        eprintln!("контроль no-clear, Roaming: {no_clear:#?}");
        eprintln!(
            "контроль no-clear: запись {LAST_VISITED} есть: {}",
            no_clear.iter().any(|h| h.contains(LAST_VISITED))
        );
        let (_rfd, rfd_name) = pick_temp_folder("rfd");
        let rfd = roaming_hits(&rfd_name);
        eprintln!("контроль rfd, Roaming: {rfd:#?}");
        if rfd.is_empty() {
            failures.push("контроль rfd: поиск не увидел путь в Roaming".to_owned());
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Окно-владелец закрыли, пока шёл запрос: окно выбора не показывается,
/// ответ — «отмена».
#[test]
fn closed_owner_window_gives_cancel_without_dialog() {
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WINDOW_STYLE,
    };
    let title = unique("Rhizomind владелец закрыт");
    // SAFETY: окно-заглушка создаётся и сразу уничтожается в этом потоке.
    let owner = unsafe {
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            &HSTRING::from("STATIC"),
            &HSTRING::from(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            None,
            None,
        )
        .expect("окно-заглушка");
        DestroyWindow(hwnd).expect("уничтожить заглушку");
        hwnd.0 as isize
    };
    // Подстраховка: если окно всё же появится — «Отмена», тест не зависнет.
    let user = press(title.clone(), IDCANCEL.0, 50);
    let chosen = show(Some(owner), &title, PRIVATE, |_| Ok(()));
    let log = user.join().expect("поток пользователя");
    assert_eq!(chosen, None);
    assert_eq!(
        log,
        vec!["окно не найдено".to_owned()],
        "окно не показывалось"
    );
}
