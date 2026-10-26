//! Окно выбора папки на Windows: опции окна (обычный прогон) и проверка
//! следов на настоящем окне (`--ignored`, отдельный шаг CI на Windows).

use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    BM_CLICK, FindWindowW, GetDlgItem, IDCANCEL, IDOK, IsWindow, PostMessageW, SendMessageW,
    SetForegroundWindow, WM_COMMAND,
};
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

/// «Пользователь» в отдельном потоке: ждёт окно с заголовком `title` и
/// нажимает кнопку выбора папки (как зонд расследования 0.6, вариант `proposal`).
fn press_select(title: String) -> std::thread::JoinHandle<Vec<String>> {
    std::thread::spawn(move || {
        let mut log = Vec::new();
        let title = HSTRING::from(title);
        let class = HSTRING::from("#32770");
        let mut window = None;
        for _ in 0..200 {
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
            let button = GetDlgItem(Some(window), IDOK.0).ok();
            let _ = PostMessageW(
                Some(window),
                WM_COMMAND,
                WPARAM(IDOK.0 as usize),
                LPARAM(button.map_or(0, |b| b.0 as isize)),
            );
            log.push("WM_COMMAND IDOK".to_owned());
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
/// `Software\Classes` (это UsrClass.dat — Local), и `%APPDATA%` (имена
/// файлов; содержимое — в недавних документах).
fn roaming_hits(name: &str) -> Vec<String> {
    let needles = needles(name);
    let appdata = PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA"));
    let recent = appdata.join(r"Microsoft\Windows\Recent");
    let mut hits = registry_hits("", &[r"Software\Classes"], &needles);
    hits.extend(file_hits(&appdata, name, &recent, &needles));
    hits
}

/// Показывает окно с настройками `settings`, «выбирает» новую временную папку
/// и возвращает её имя и ответ окна.
fn pick_temp_folder(settings: Settings, label: &str) -> (tempfile::TempDir, String) {
    let name = unique(&format!("rhz-t08-{label}"));
    let folder = tempfile::Builder::new()
        .prefix(&name)
        .rand_bytes(0)
        .tempdir()
        .expect("временная папка");
    let title = unique("Rhizomind проверка окна");
    let user = press_select(title.clone());
    let chosen = show(None, &title, settings, Some(folder.path()));
    let log = user.join().expect("поток пользователя");
    eprintln!("[{label}] окно: {log:?}; ответ: {chosen:?}");
    let expected = std::fs::canonicalize(folder.path()).expect("канонический путь");
    let chosen = chosen.map(|p| std::fs::canonicalize(p).expect("канонический ответ"));
    assert_eq!(
        chosen.as_deref(),
        Some(expected.as_path()),
        "окно вернуло выбранную папку"
    );
    // Недавние документы пишет Explorer.EXE по вызову окна — не сразу.
    std::thread::sleep(Duration::from_secs(5));
    (folder, name)
}

/// Настоящее окно на настоящем рабочем столе: в обычный прогон не входит.
/// CI на Windows: `cargo test -p rhizomind --lib dialogs::folder_windows -- --ignored`.
/// С `RHIZOMIND_T08_CONTROL=1` — ещё и контроль: окно с опциями rfd пишет путь
/// в перемещаемый профиль, и поиск его находит (на рабочей машине не включать).
#[test]
#[ignore = "показывает окно ОС; отдельный шаг CI на Windows"]
fn chosen_folder_path_does_not_reach_roaming_profile() {
    let (_folder, name) = pick_temp_folder(PRIVATE, "private");

    let roaming = roaming_hits(&name);
    eprintln!("Roaming (NTUSER.DAT, %APPDATA%): {roaming:#?}");
    let local = registry_hits(
        r"Software\Classes\Local Settings\Software\Microsoft\Windows\Shell",
        &[],
        &needles(&name),
    );
    eprintln!("Local (UsrClass.dat, ShellBags): {local:#?}");
    assert!(
        roaming.is_empty(),
        "путь к выбранной папке в перемещаемом профиле: {roaming:#?}"
    );
    // ShellBags (Local) по расследованию 0.6 хранят путь при любых опциях —
    // только в журнал: это допустимая локальная запись (инвариант 2).

    if std::env::var_os("RHIZOMIND_T08_CONTROL").is_some_and(|v| v == "1") {
        let rfd = Settings {
            options: FOS_PICKFOLDERS,
            client_guid: None,
            clear_client_data: false,
        };
        let (_control, control_name) = pick_temp_folder(rfd, "rfd");
        let control = roaming_hits(&control_name);
        eprintln!("контроль (опции rfd), Roaming: {control:#?}");
        assert!(
            !control.is_empty(),
            "контроль поиска: окно с опциями rfd пишет путь в Roaming — поиск его не увидел"
        );
    }
}
