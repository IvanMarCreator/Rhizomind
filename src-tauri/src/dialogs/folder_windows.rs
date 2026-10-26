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
use std::path::PathBuf;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FILEOPENDIALOGOPTIONS, FOS_DONTADDTORECENT, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
    FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::IsWindow;
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
/// передаётся между потоками). `None` — отмена, окно-владелец уже закрыто или
/// окно не удалось показать (сбой пишется в stderr с HRESULT, без пути).
/// В тестах крейта вместо окна — подмена ответов (`dialogs::test_answers`).
#[cfg_attr(test, allow(dead_code))]
pub fn pick_folder(owner: Option<isize>, title: &str) -> Option<PathBuf> {
    show(owner, title, PRIVATE, |_| Ok(()))
}

/// Подготовка окна перед показом. В программе — ничего; в тесте — ввод пути
/// в поле имени, как это сделал бы пользователь.
type Prepare = dyn FnOnce(&IFileOpenDialog) -> windows::core::Result<()> + Send;

/// Показывает окно в отдельном STA-потоке и ждёт ответа.
fn show(
    owner: Option<isize>,
    title: &str,
    settings: Settings,
    prepare: impl FnOnce(&IFileOpenDialog) -> windows::core::Result<()> + Send + 'static,
) -> Option<PathBuf> {
    let title = HSTRING::from(title);
    let prepare: Box<Prepare> = Box::new(prepare);
    let answer = std::thread::Builder::new()
        .name("rhizomind-folder-dialog".into())
        .spawn(move || {
            // SAFETY: COM инициализируется в этом потоке и снимается в нём же,
            // после того как все объекты COM внутри `show_sta` освобождены.
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)
                    .ok()
                    .map_err(|e| Failure("CoInitializeEx", e))?;
                let chosen = show_sta(owner, &title, settings, prepare);
                CoUninitialize();
                chosen
            }
        })
        .ok()?
        .join()
        .ok()?;
    match answer {
        Ok(chosen) => chosen,
        Err(failure) => {
            eprintln!("Rhizomind: окно выбора папки не показано: {failure}");
            None
        }
    }
}

/// Сбой COM: шаг и HRESULT (без пути и имён).
#[derive(Debug)]
pub(super) struct Failure(&'static str, windows::core::Error);

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: HRESULT {:#010x}", self.0, self.1.code().0 as u32)
    }
}

/// Ответ окна «Отмена» (`HRESULT_FROM_WIN32(ERROR_CANCELLED)`).
const CANCELLED: windows::core::HRESULT = windows::core::HRESULT(0x8007_04C7_u32 as i32);

/// Показ окна в потоке с инициализированным STA.
unsafe fn show_sta(
    owner: Option<isize>,
    title: &HSTRING,
    settings: Settings,
    prepare: Box<Prepare>,
) -> Result<Option<PathBuf>, Failure> {
    let owner = owner.map(|raw| HWND(raw as *mut core::ffi::c_void));
    // SAFETY: вызывается из `show` в потоке с инициализированным COM;
    // IsWindow только проверяет описатель.
    unsafe {
        if let Some(owner) = owner
            && !IsWindow(Some(owner)).as_bool()
        {
            // Окно-владелец закрыли, пока шёл запрос: выбор не показываем.
            return Ok(None);
        }
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| Failure("CoCreateInstance", e))?;
        if let Some(guid) = settings.client_guid {
            dialog
                .SetClientGuid(&guid)
                .map_err(|e| Failure("SetClientGuid", e))?;
        }
        dialog
            .SetOptions(settings.options)
            .map_err(|e| Failure("SetOptions", e))?;
        dialog.SetTitle(title).map_err(|e| Failure("SetTitle", e))?;
        prepare(&dialog).map_err(|e| Failure("подготовка", e))?;
        let shown = dialog.Show(owner);
        let chosen = match shown {
            Ok(()) => dialog
                .GetResult()
                .map_err(|e| Failure("GetResult", e))
                .and_then(|item| file_system_path(&item)),
            Err(e) if e.code() == CANCELLED => Ok(None),
            Err(e) => Err(Failure("Show", e)),
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
unsafe fn file_system_path(item: &IShellItem) -> Result<Option<PathBuf>, Failure> {
    // SAFETY: строку выделяет оболочка (CoTaskMemAlloc); освобождаем после копии.
    unsafe {
        let raw = item
            .GetDisplayName(SIGDN_FILESYSPATH)
            .map_err(|e| Failure("GetDisplayName", e))?;
        let path = PathBuf::from(OsString::from_wide(raw.as_wide()));
        CoTaskMemFree(Some(raw.0 as *const core::ffi::c_void));
        Ok(Some(path))
    }
}

#[cfg(test)]
mod tests;
