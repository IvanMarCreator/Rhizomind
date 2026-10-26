//! Своё окно выбора папки на Windows (задача 0.8, решение владельца 56).
//!
//! Системное окно через `tauri-plugin-dialog` → rfd ставит ровно
//! `FOS_PICKFOLDERS` и пишет путь к выбранной папке в перемещаемую часть
//! профиля: `ComDlg32\LastVisitedPidlMRU`, `OpenSavePidlMRU`, недавние
//! документы (`RecentDocs`, ярлык в `%APPDATA%\Microsoft\Windows\Recent`).
//! Это противоречит решению 46 (инвариант 2). Здесь то же окно ОС
//! (`IFileOpenDialog`), но с опциями, которые расследование 0.6 проверило
//! опытом (`docs/research/2026-10-26-traces-investigation.md`, вариант
//! `proposal`):
//!
//! - `FOS_DONTADDTORECENT` — без недавних документов, ярлыка и
//!   `OpenSavePidlMRU`;
//! - постоянный GUID клиента ([`CLIENT_GUID`]) и `ClearClientData` после
//!   закрытия окна — стирает запись `LastVisitedPidlMRU`. `ClearClientData`
//!   возвращает `E_FAIL`, но запись стирает, поэтому ответ не проверяется.
//!
//! Остаются размер окна (`CIDSizeMRU`, по GUID клиента) и настройки панели
//! навигации — в Roaming, без пути; ShellBags с путём — в Local
//! (`UsrClass.dat`). Учтено в `docs/SECURITY.md`, «Записи вне хранилища».
//!
//! COM инициализируется на время показа в отдельном STA-потоке: вызывающий
//! поток (рабочий поток `spawn_blocking`) состояние COM не меняет.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FILEOPENDIALOGOPTIONS, FOS_DONTADDTORECENT, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
    FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName,
    SIGDN_FILESYSPATH,
};
use windows::core::{GUID, HSTRING};

/// GUID клиента окна выбора папки Rhizomind. Постоянный: по нему ОС хранит
/// состояние окна отдельно от других программ, и `ClearClientData` стирает
/// именно его. Не менять — иначе останется запись под старым GUID.
pub(super) const CLIENT_GUID: GUID = GUID::from_u128(0x5c0e_8a2d_3f71_4b6e_9d42_a1b7_c3e9_f085);

/// Как показывать окно.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Settings {
    /// Опции `IFileDialog::SetOptions`.
    pub options: FILEOPENDIALOGOPTIONS,
    /// GUID клиента (`SetClientGuid`).
    pub client_guid: Option<GUID>,
    /// Вызвать `ClearClientData` после закрытия окна.
    pub clear_client_data: bool,
}

/// Окно Rhizomind: путь к выбранной папке не попадает в перемещаемый профиль.
pub(super) const PRIVATE: Settings = Settings {
    options: FILEOPENDIALOGOPTIONS(
        FOS_PICKFOLDERS.0 | FOS_DONTADDTORECENT.0 | FOS_FORCEFILESYSTEM.0 | FOS_PATHMUSTEXIST.0,
    ),
    client_guid: Some(CLIENT_GUID),
    clear_client_data: true,
};

/// Окно выбора папки, модальное к окну `owner` (HWND как число: `HWND` не
/// передаётся между потоками). `None` — отмена или окно не удалось показать.
/// В тестах крейта вместо окна — подмена ответов (`dialogs::test_answers`).
#[cfg_attr(test, allow(dead_code))]
pub fn pick_folder(owner: Option<isize>, title: &str) -> Option<PathBuf> {
    show(owner, title, PRIVATE, None)
}

/// Показывает окно в отдельном STA-потоке и ждёт ответа. `preset` — только
/// для теста: окно открывается в родительской папке с именем выбранной папки
/// в поле ввода, как если бы пользователь до неё дошёл.
fn show(
    owner: Option<isize>,
    title: &str,
    settings: Settings,
    preset: Option<&Path>,
) -> Option<PathBuf> {
    let title = HSTRING::from(title);
    let preset = preset.map(Path::to_path_buf);
    std::thread::Builder::new()
        .name("rhizomind-folder-dialog".into())
        .spawn(move || {
            // SAFETY: COM инициализируется в этом потоке и снимается в нём же,
            // после того как все объекты COM внутри `show_sta` освобождены.
            unsafe {
                if CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).is_err()
                {
                    return None;
                }
                let chosen = show_sta(owner, &title, settings, preset.as_deref());
                CoUninitialize();
                chosen
            }
        })
        .ok()?
        .join()
        .ok()
        .flatten()
}

/// Показ окна в потоке с инициализированным STA.
unsafe fn show_sta(
    owner: Option<isize>,
    title: &HSTRING,
    settings: Settings,
    preset: Option<&Path>,
) -> Option<PathBuf> {
    // SAFETY: вызывается из `show` в потоке с инициализированным COM.
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        if let Some(guid) = settings.client_guid {
            dialog.SetClientGuid(&guid).ok()?;
        }
        dialog.SetOptions(settings.options).ok()?;
        dialog.SetTitle(title).ok()?;
        if let Some(folder) = preset {
            let parent = folder.parent()?;
            let leaf = folder.file_name()?;
            let item: IShellItem =
                SHCreateItemFromParsingName(&HSTRING::from(parent.as_os_str()), None).ok()?;
            dialog.SetFolder(&item).ok()?;
            dialog.SetFileName(&HSTRING::from(leaf)).ok()?;
        }
        let owner = owner.map(|raw| HWND(raw as *mut core::ffi::c_void));
        let chosen = match dialog.Show(owner) {
            Ok(()) => dialog
                .GetResult()
                .ok()
                .and_then(|item| file_system_path(&item)),
            Err(_) => None,
        };
        if settings.clear_client_data {
            // Возвращает E_FAIL, но запись LastVisitedPidlMRU стирает
            // (расследование 0.6, Process Monitor: RegDeleteValue).
            let _ = dialog.ClearClientData();
        }
        chosen
    }
}

/// Путь в файловой системе для элемента оболочки.
unsafe fn file_system_path(item: &IShellItem) -> Option<PathBuf> {
    // SAFETY: строку выделяет оболочка (CoTaskMemAlloc); освобождаем после копии.
    unsafe {
        let raw = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PathBuf::from(OsString::from_wide(raw.as_wide()));
        CoTaskMemFree(Some(raw.0 as *const core::ffi::c_void));
        Some(path)
    }
}

#[cfg(test)]
mod tests;
