//! Список хранилищ (`vaults.json`, задача 1.3).
//!
//! Единственный файл приложения в профиле ОС:
//! `dirs::data_local_dir()/Rhizomind/vaults.json` (спецификация 0.1, С4;
//! решение 46) — на Windows `%LOCALAPPDATA%\Rhizomind\vaults.json`, не
//! перемещаемый профиль `%APPDATA%\Roaming`. В нём только версия формата,
//! идентификаторы, имена и пути хранилищ, последнее открытое хранилище и
//! размер и положение окна каждого хранилища — ни имён, ни содержимого
//! заметок (инвариант 2). Учтён в `docs/SECURITY.md`, «Записи вне хранилища».
//!
//! - Запись атомарная: временный файл в той же папке (`tempfile`), сброс на
//!   диск, замена `vaults.json`.
//! - Испорченный файл (не JSON, другая версия формата, лишние поля, пустые
//!   или повторяющиеся идентификаторы) не роняет приложение и не теряется:
//!   он копируется рядом как `vaults.broken-<мс от 1970>.json`, список
//!   начинается пустым. Не удалось скопировать — испорченный файл не
//!   перезаписывается, список живёт только в памяти до выхода.
//!
//! Открытие хранилища в окне и фокус на уже открытое — модуль окон
//! (`src-tauri/src/windows/`); этот модуль только помнит. Выбор папки —
//! системный диалог ядра (`crate::dialogs::pick_folder`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};

use crate::commands::{CoreError, CoreResult, ErrorCode, VaultId, VaultSummary};

#[cfg(test)]
mod tests;

/// Версия формата `vaults.json`.
pub const FORMAT_VERSION: u32 = 1;

/// Имя папки приложения в `dirs::data_local_dir()`.
pub const APP_DIR: &str = "Rhizomind";

/// Имя файла списка хранилищ.
pub const FILE_NAME: &str = "vaults.json";

/// Начало имени копии испорченного файла: `vaults.broken-<мс>.json`.
pub const BROKEN_PREFIX: &str = "vaults.broken-";

/// Путь к `vaults.json` в профиле текущего пользователя:
/// `dirs::data_local_dir()/Rhizomind/vaults.json` (решение 46).
pub fn profile_file() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join(APP_DIR).join(FILE_NAME))
}

/// Размер и положение окна хранилища в логических пикселях.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowState {
    /// Ширина области окна.
    pub width: u32,
    /// Высота области окна.
    pub height: u32,
    /// Левый край окна на экране.
    pub x: i32,
    /// Верхний край окна на экране.
    pub y: i32,
    /// Окно развёрнуто на весь экран (размер и положение — до разворота).
    pub maximized: bool,
}

/// Хранилище в файле.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredVault {
    id: VaultId,
    name: String,
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window: Option<WindowState>,
}

impl StoredVault {
    fn summary(&self) -> VaultSummary {
        VaultSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            path: self.path.clone(),
        }
    }
}

/// Содержимое `vaults.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    format_version: u32,
    vaults: Vec<StoredVault>,
    last_opened: Option<VaultId>,
}

impl Default for RegistryFile {
    fn default() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            vaults: Vec::new(),
            last_opened: None,
        }
    }
}

impl RegistryFile {
    /// Разбор с проверкой; `None` — файл испорчен.
    fn parse(text: &str) -> Option<Self> {
        let mut file: Self = serde_json::from_str(text).ok()?;
        if file.format_version != FORMAT_VERSION {
            return None;
        }
        let mut ids = std::collections::BTreeSet::new();
        for vault in &file.vaults {
            if vault.id.0.is_empty() || vault.path.is_empty() || !ids.insert(vault.id.0.clone()) {
                return None;
            }
        }
        if let Some(last) = &file.last_opened
            && !ids.contains(&last.0)
        {
            file.last_opened = None;
        }
        Some(file)
    }

    fn position(&self, id: &VaultId) -> CoreResult<usize> {
        self.vaults
            .iter()
            .position(|vault| vault.id == *id)
            .ok_or_else(|| CoreError::new(ErrorCode::VaultNotFound))
    }
}

/// Загруженный список.
#[derive(Debug)]
struct Loaded {
    file: RegistryFile,
    /// `false` — испорченный файл не удалось скопировать: не перезаписываем.
    writable: bool,
}

/// Содержимое списка для окна «Управление хранилищами».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntries {
    /// Хранилища в порядке списка.
    pub vaults: Vec<VaultSummary>,
    /// Последнее открытое хранилище.
    pub last_opened: Option<VaultId>,
}

/// Список хранилищ в профиле ОС.
///
/// Потокобезопасен: команды разных окон обращаются к одному экземпляру.
/// Файл читается при первом обращении; каждое изменение сразу записывается
/// на диск атомарно. `Registry::default()` — список только в памяти (тесты).
#[derive(Debug, Default)]
pub struct Registry {
    /// Путь к `vaults.json`; `None` — список только в памяти.
    file: Option<PathBuf>,
    state: Mutex<Option<Loaded>>,
    #[cfg(test)]
    fail_backup: std::sync::atomic::AtomicBool,
}

impl Registry {
    /// Список в профиле текущего пользователя ОС ([`profile_file`]); его
    /// отсутствие — пустой список (первый запуск). Папки профиля нет — список
    /// только в памяти.
    pub fn from_profile() -> Self {
        Self {
            file: profile_file(),
            ..Self::default()
        }
    }

    /// Список в файле `file` (тесты, тестовая сборка).
    pub fn at(file: PathBuf) -> Self {
        Self {
            file: Some(file),
            ..Self::default()
        }
    }

    /// Путь к файлу списка; `None` — список только в памяти.
    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    #[cfg(test)]
    fn fail_backup_for_tests(&self) {
        self.fail_backup
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Все хранилища и последнее открытое.
    pub fn entries(&self) -> CoreResult<RegistryEntries> {
        let state = self.loaded();
        let file = &state.as_ref().expect("загружено").file;
        Ok(RegistryEntries {
            vaults: file.vaults.iter().map(StoredVault::summary).collect(),
            last_opened: file.last_opened.clone(),
        })
    }

    /// Хранилище по идентификатору; нет в списке — `vault_not_found`.
    pub fn find(&self, id: &VaultId) -> CoreResult<VaultSummary> {
        let state = self.loaded();
        let file = &state.as_ref().expect("загружено").file;
        file.position(id).map(|i| file.vaults[i].summary())
    }

    /// Последнее открытое хранилище.
    pub fn last_opened(&self) -> Option<VaultSummary> {
        let state = self.loaded();
        let file = &state.as_ref().expect("загружено").file;
        let id = file.last_opened.as_ref()?;
        file.position(id).ok().map(|i| file.vaults[i].summary())
    }

    /// Запомненные размер и положение окна хранилища.
    pub fn window(&self, id: &VaultId) -> Option<WindowState> {
        let state = self.loaded();
        let file = &state.as_ref().expect("загружено").file;
        file.position(id).ok().and_then(|i| file.vaults[i].window)
    }

    /// Добавляет папку как хранилище (или возвращает уже известное с тем же
    /// путём). Папка должна существовать (`vault_unavailable`); имя хранилища
    /// — имя папки. Папку `.rhizomind/` создаёт модуль `vault` при открытии.
    pub fn add(&self, root: &Path) -> CoreResult<VaultSummary> {
        let unavailable = || CoreError::new(ErrorCode::VaultUnavailable);
        let root = dunce::canonicalize(root).map_err(|_| unavailable())?;
        if !root.is_dir() {
            return Err(unavailable());
        }
        let path = root
            .to_str()
            .ok_or_else(|| CoreError::new(ErrorCode::InvalidPath))?
            .to_owned();
        let name = root
            .file_name()
            .and_then(|name| name.to_str())
            .map_or_else(|| path.clone(), str::to_owned);
        self.change(|file| {
            if let Some(known) = file.vaults.iter().find(|vault| vault.path == path) {
                return Ok(known.summary());
            }
            let id = loop {
                let id = VaultId(format!("{:016x}", crate::webview_traces::random_u64()));
                if file.vaults.iter().all(|vault| vault.id != id) {
                    break id;
                }
            };
            let vault = StoredVault {
                id,
                name,
                path,
                window: None,
            };
            let summary = vault.summary();
            file.vaults.push(vault);
            Ok(summary)
        })
    }

    /// Убирает хранилище из списка. Файлы на диске не трогаются.
    pub fn remove(&self, id: &VaultId) -> CoreResult<()> {
        self.change(|file| {
            let index = file.position(id)?;
            file.vaults.remove(index);
            if file.last_opened.as_ref() == Some(id) {
                file.last_opened = None;
            }
            Ok(())
        })
    }

    /// Запоминает хранилище как последнее открытое.
    pub fn mark_opened(&self, id: &VaultId) -> CoreResult<()> {
        self.change(|file| {
            file.position(id)?;
            file.last_opened = Some(id.clone());
            Ok(())
        })
    }

    /// Запоминает размер и положение окна хранилища.
    pub fn save_window(&self, id: &VaultId, window: WindowState) -> CoreResult<()> {
        self.change(|file| {
            let index = file.position(id)?;
            file.vaults[index].window = Some(window);
            Ok(())
        })
    }

    /// Изменение списка: на копии, запись на диск, затем в памяти. Ошибка
    /// записи — `io_failed`, список в памяти не меняется.
    fn change<T>(&self, edit: impl FnOnce(&mut RegistryFile) -> CoreResult<T>) -> CoreResult<T> {
        let mut state = self.loaded();
        let loaded = state.as_mut().expect("загружено");
        let mut next = loaded.file.clone();
        let result = edit(&mut next)?;
        if next != loaded.file {
            if loaded.writable
                && let Some(path) = &self.file
            {
                write_atomically(path, &next).map_err(|_| CoreError::new(ErrorCode::IoFailed))?;
            }
            loaded.file = next;
        }
        Ok(result)
    }

    /// Список, загруженный при первом обращении.
    fn loaded(&self) -> MutexGuard<'_, Option<Loaded>> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.is_none() {
            *state = Some(self.load());
        }
        state
    }

    fn load(&self) -> Loaded {
        let fresh = |writable| Loaded {
            file: RegistryFile::default(),
            writable,
        };
        let Some(path) = &self.file else {
            return fresh(true);
        };
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return fresh(true),
            // Файл есть, но не читается: не перезаписываем его.
            Err(_) => return fresh(false),
        };
        if let Some(file) = std::str::from_utf8(&bytes)
            .ok()
            .and_then(RegistryFile::parse)
        {
            return Loaded {
                file,
                writable: true,
            };
        }
        // Испорчен: копия рядом, список с нуля.
        let copied = self.copy_aside(path, &bytes).is_ok();
        fresh(copied)
    }

    fn copy_aside(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        #[cfg(test)]
        if self.fail_backup.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(std::io::Error::other("тест: копия не удалась"));
        }
        let dir = path.parent().unwrap_or(Path::new("."));
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let mut copy = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(format!("{BROKEN_PREFIX}{millis}.json")))?;
        copy.write_all(bytes)?;
        copy.sync_all()
    }
}

/// Атомарная запись: временный файл в той же папке, сброс на диск, замена.
fn write_atomically(path: &Path, file: &RegistryFile) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut text = serde_json::to_vec_pretty(file).map_err(std::io::Error::other)?;
    text.push(b'\n');
    let mut temp = tempfile::Builder::new()
        .prefix(".vaults-")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    temp.write_all(&text)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}
