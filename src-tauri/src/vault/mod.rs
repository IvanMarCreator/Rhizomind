//! Хранилище: контур, файлы и служебные данные (задача 1.2).
//!
//! Модуль — единственное место ядра, которое обращается к файлам хранилища.
//! Всё, что приходит от интерфейса, проходит один порядок проверки контура
//! ([`contour`], исследование крейтов — «Контур хранилища»):
//! 1. каждый компонент пути — [`check_component`]; служебная папка
//!    `.rhizomind` недоступна обычным командам (по ключу имени и коротким
//!    именам 8.3);
//! 2. корень канонизирован один раз в [`Vault::open`] (`dunce::canonicalize`);
//! 3. каждый промежуточный компонент и сам элемент — `symlink_metadata`:
//!    символическая ссылка или точка соединения NTFS → `outside_vault`;
//! 4. итоговый путь начинается с корня;
//! 5. после открытия — итоговый путь по дескриптору (против гонки
//!    «проверил — открыл»).
//!
//! Запись заметки атомарная (временный файл в `.rhizomind/tmp/` → замена),
//! с проверкой ожидаемой версии файла. Удаление — в корзину ОС; на томе без
//! корзины — безвозвратно и только после отдельного подтверждения
//! (инвариант 4). Служебные данные — `.rhizomind/settings.json`
//! ([`SettingsFile`]) и `.rhizomind/workspace.json`.
//!
//! **Дата создания заметки** — источник «самой старой» заметки при
//! дубликатах имён (С3.4, [`NoteRef::created_ms`]). Берётся из файловой
//! системы, поэтому атомарная запись её сохраняет: на Windows замена через
//! `ReplaceFileW` (сохраняет дату создания заменяемого файла). На Linux
//! времени создания не задать — дата создания после записи может
//! сдвинуться; на macOS перенос даты создания (`setattrlist`) пока не
//! сделан. Это известное ограничение до перехода на эти ОС (ADR 0006,
//! уточнение 25.10.2026).

pub(crate) mod atomic;
mod contour;
mod settings;
mod text;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::commands::{
    CoreError, CoreResult, ErrorCode, FileVersion, NameCheck, NoteReadResponse, NoteWriteResponse,
    ReadOnlyReason, Settings, TrashOutcome, TreeEntry, VaultPath, Workspace,
};

pub use contour::{SERVICE_DIR, check_component};
pub use settings::{SETTINGS_FORMAT_VERSION, SettingsFile};

use contour::Contour;

/// Временные файлы атомарной записи — `.rhizomind/tmp/`.
const TMP_DIR: &str = "tmp";
/// Настройки хранилища — `.rhizomind/settings.json`.
const SETTINGS_FILE: &str = "settings.json";
/// Служебный файл больше этого не читается (правка руками) — значения по
/// умолчанию.
const MAX_SERVICE_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// Заметка в списке, по которому разрешаются ссылки и идёт быстрый переход.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteRef {
    /// Путь заметки (с `.md`).
    pub path: VaultPath,
    /// Дата создания файла, миллисекунды UTC — для выбора самой старой
    /// заметки при дубликатах имён.
    pub created_ms: i64,
}

/// Своя запись ядра: путь, хэш и размер новой версии файла. Сообщается
/// **до** замены файла (ADR 0006, уточнение 25.10.2026).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnWrite {
    /// Путь заметки.
    pub path: VaultPath,
    /// Хэш `blake3` нового содержимого.
    pub hash: String,
    /// Размер нового содержимого.
    pub size: u64,
}

/// Получатель своих записей.
pub type OwnWriteHook = Box<dyn Fn(&OwnWrite) + Send + Sync + 'static>;

/// Открытое хранилище: корень и операции над файлами внутри контура.
///
/// Все методы принимают пути [`VaultPath`] от интерфейса как есть и сами
/// проверяют их компоненты; пределы длины уже проверены слоем команд.
/// Методы блокируют поток (диск, корзина) — команды вызывают их вне
/// главного потока.
#[derive(Debug)]
pub struct Vault {
    contour: Contour,
    /// `.rhizomind/` — канонический путь, проверен при открытии.
    service: PathBuf,
}

impl Vault {
    /// Создаёт папку нового хранилища `parent/name` (не существующую).
    /// Имя уже проверено правилами имён и [`check_component`]; возвращает
    /// путь к созданной папке. `.rhizomind/` создаёт [`Vault::open`].
    pub fn create(_parent: &Path, _name: &str) -> CoreResult<PathBuf> {
        Err(CoreError::not_implemented())
    }

    /// Открывает папку как хранилище: канонизирует корень, создаёт
    /// `.rhizomind/` и `.rhizomind/tmp/`, убирает остатки временных файлов.
    /// Служебная папка или её `tmp` — ссылка (в том числе точка соединения)
    /// → `outside_vault`: запись через них ушла бы за контур.
    pub fn open(root: &Path) -> CoreResult<Self> {
        let root = dunce::canonicalize(root).map_err(|_| unavailable())?;
        if !std::fs::metadata(&root)
            .map_err(|_| unavailable())?
            .is_dir()
        {
            return Err(unavailable());
        }
        let contour = Contour::new(root.clone());
        let service = root.join(SERVICE_DIR);
        for dir in [service.clone(), service.join(TMP_DIR)] {
            match std::fs::symlink_metadata(&dir) {
                Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                    return Err(contour::outside_vault());
                }
                Ok(_) => {}
                Err(_) => std::fs::create_dir(&dir).map_err(io_error)?,
            }
            let handle = contour::open_dir(&dir).map_err(io_error)?;
            contour.check_opened(&handle, &dir, true)?;
        }
        let vault = Self { contour, service };
        vault.clean_tmp();
        Ok(vault)
    }

    /// Корень хранилища (канонический).
    pub fn root(&self) -> &Path {
        self.contour.root()
    }

    /// Убирает остатки временных файлов прерванных записей.
    fn clean_tmp(&self) {
        if let Ok(entries) = std::fs::read_dir(self.tmp_dir()) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|t| t.is_file()) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    fn tmp_dir(&self) -> PathBuf {
        self.service.join(TMP_DIR)
    }

    /// Все папки и файлы хранилища, кроме `.rhizomind/` и имён с точки в
    /// начале. Плоский список; дерево и сортировку строит интерфейс.
    pub fn tree(&self) -> CoreResult<Vec<TreeEntry>> {
        Err(CoreError::not_implemented())
    }

    /// Все заметки хранилища с датой создания — для модулей `links` и
    /// `switcher`. «Самая старая» из заметок с одним именем — с наименьшей
    /// `created_ms`; при равной дате — первая по пути в естественном
    /// алфавитном порядке (С3.4).
    pub fn notes(&self) -> CoreResult<Vec<NoteRef>> {
        Err(CoreError::not_implemented())
    }

    /// Читает заметку: текст без BOM с переносами `\n`, версию файла.
    /// Больше лимита из настроек или не UTF-8 — только чтение.
    pub fn read_note(&self, path: &VaultPath) -> CoreResult<NoteReadResponse> {
        let abs = self.contour.resolve(path)?;
        require_note_name(path)?;
        let (bytes, version) = self.read_existing_note(&abs)?;
        let decoded = text::decode(&bytes);
        let limit = self.read_settings()?.max_edit_size_bytes;
        let read_only = if decoded.invalid_utf8 {
            Some(ReadOnlyReason::InvalidUtf8)
        } else if version.size > limit {
            Some(ReadOnlyReason::TooLarge { limit_bytes: limit })
        } else {
            None
        };
        Ok(NoteReadResponse {
            text: decoded.text,
            version,
            read_only,
        })
    }

    /// Байты и версия существующей заметки; итоговый путь проверен по
    /// дескриптору. Папка — `not_a_note`, нет файла — `not_found`.
    fn read_existing_note(&self, abs: &Path) -> CoreResult<(Vec<u8>, FileVersion)> {
        let meta = std::fs::metadata(abs).map_err(io_error)?;
        if meta.is_dir() {
            return Err(CoreError::new(ErrorCode::NotANote));
        }
        let mut file = File::open(abs).map_err(io_error)?;
        self.contour.check_opened(&file, abs, false)?;
        let mut bytes = Vec::with_capacity(meta.len() as usize);
        file.read_to_end(&mut bytes).map_err(io_error)?;
        let meta = file.metadata().map_err(io_error)?;
        let version = version_of(&bytes, &meta);
        Ok((bytes, version))
    }

    /// Записывает заметку атомарно по правилу «байт в байт»
    /// (`commands::note`): BOM и переносы — из файла на диске; текст не
    /// изменился — файл не пишется. Версия на диске не равна `expected` —
    /// изменения снаружи не затираются: версия с диска сохраняется копией
    /// рядом, текст из редактора — в заметку (`conflict_saved`, С3.6).
    /// Файла нет — `not_found`. Дата создания сохраняется (см. описание
    /// модуля).
    /// Регистрирует получателя своих записей (наблюдатель, задача 1.4).
    pub fn set_own_write_hook(&self, _hook: OwnWriteHook) {}

    /// Своя ли это недавняя запись (путь, хэш и размер новой версии).
    pub fn is_own_write(&self, _path: &VaultPath, _hash: &str, _size: u64) -> bool {
        false
    }

    pub fn write_note(
        &self,
        path: &VaultPath,
        _text: &str,
        _expected: &FileVersion,
    ) -> CoreResult<NoteWriteResponse> {
        self.contour.resolve(path)?;
        Err(CoreError::not_implemented())
    }

    /// Создаёт пустую заметку `name` (полное имя с `.md`; UTF-8 без BOM,
    /// переносы LF) в папке `dir`.
    pub fn create_note(&self, dir: &VaultPath, name: &str) -> CoreResult<VaultPath> {
        self.contour.resolve(&child(dir, name))?;
        Err(CoreError::not_implemented())
    }

    /// Создаёт папку `name` в папке `dir`.
    pub fn create_folder(&self, dir: &VaultPath, name: &str) -> CoreResult<VaultPath> {
        self.contour.resolve(&child(dir, name))?;
        Err(CoreError::not_implemented())
    }

    /// Совпадение имени с соседями в папке `dir` по ключу имени
    /// (`names::key`); правила самого имени уже проверены слоем команд.
    /// `renaming` — переименовываемый элемент: он не совпадение сам с собой,
    /// поэтому смена только регистра разрешена.
    pub fn check_new_name(
        &self,
        dir: &VaultPath,
        _name: &str,
        _renaming: Option<&VaultPath>,
    ) -> CoreResult<NameCheck> {
        self.contour.resolve(dir)?;
        Err(CoreError::not_implemented())
    }

    /// Переименовывает файл или папку; возвращает новый путь. Смена только
    /// регистра разрешена (на Windows — через промежуточное имя).
    pub fn rename(&self, path: &VaultPath, _new_name: &str) -> CoreResult<VaultPath> {
        require_not_root(path)?;
        self.contour.resolve(path)?;
        Err(CoreError::not_implemented())
    }

    /// Перемещает файл или папку в `new_dir`; возвращает новый путь.
    pub fn move_entry(&self, path: &VaultPath, new_dir: &VaultPath) -> CoreResult<VaultPath> {
        require_not_root(path)?;
        self.contour.resolve(path)?;
        self.contour.resolve(new_dir)?;
        Err(CoreError::not_implemented())
    }

    /// Удаляет в корзину ОС. На томе без корзины спрашивает
    /// `confirm_permanent` (окно подтверждения ядра) и удаляет безвозвратно
    /// только при согласии.
    pub fn trash(
        &self,
        path: &VaultPath,
        _confirm_permanent: &dyn Fn(&VaultPath) -> bool,
    ) -> CoreResult<TrashOutcome> {
        require_not_root(path)?;
        self.contour.resolve(path)?;
        Err(CoreError::not_implemented())
    }

    /// Настройки хранилища из `.rhizomind/settings.json`
    /// ([`SettingsFile::to_settings`]); файла нет или он не читается как
    /// JSON — значения по умолчанию.
    pub fn read_settings(&self) -> CoreResult<Settings> {
        Ok(self.read_settings_file()?.to_settings())
    }

    fn read_settings_file(&self) -> CoreResult<SettingsFile> {
        Ok(self
            .read_service_file(SETTINGS_FILE)?
            .and_then(|bytes| SettingsFile::parse(&bytes))
            .unwrap_or_default())
    }

    /// Служебный файл `.rhizomind/<name>`; нет файла — `None`. Итоговый путь
    /// проверен по дескриптору; слишком большой файл не читается (`None`).
    fn read_service_file(&self, name: &str) -> CoreResult<Option<Vec<u8>>> {
        let path = self.service.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(contour::outside_vault()),
            Ok(meta) if !meta.is_file() || meta.len() > MAX_SERVICE_FILE_BYTES => return Ok(None),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_error(e)),
        }
        let file = File::open(&path).map_err(io_error)?;
        self.contour.check_opened(&file, &path, true)?;
        let mut bytes = Vec::new();
        file.take(MAX_SERVICE_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        Ok((bytes.len() as u64 <= MAX_SERVICE_FILE_BYTES).then_some(bytes))
    }

    /// Записывает настройки хранилища атомарно ([`SettingsFile::update`]).
    pub fn write_settings(&self, _settings: &Settings) -> CoreResult<()> {
        Err(CoreError::not_implemented())
    }

    /// Раскладка окна из `.rhizomind/workspace.json`; файла нет — `None`.
    pub fn read_workspace(&self) -> CoreResult<Option<Workspace>> {
        Err(CoreError::not_implemented())
    }

    /// Записывает раскладку окна атомарно.
    pub fn write_workspace(&self, _workspace: &Workspace) -> CoreResult<()> {
        Err(CoreError::not_implemented())
    }
}

/// Путь `dir/name`.
fn child(dir: &VaultPath, name: &str) -> VaultPath {
    VaultPath::from_components(dir.components().iter().cloned().chain([name.to_owned()]))
}

/// Операция не над корнем хранилища.
fn require_not_root(path: &VaultPath) -> CoreResult<()> {
    if path.components().is_empty() {
        Err(contour::invalid_path())
    } else {
        Ok(())
    }
}

/// Имя пути — имя заметки (`.md`).
fn require_note_name(path: &VaultPath) -> CoreResult<()> {
    if path.file_name().is_some_and(crate::names::is_note) {
        Ok(())
    } else {
        Err(CoreError::new(ErrorCode::NotANote))
    }
}

/// Версия файла по содержимому и метаданным того же дескриптора.
fn version_of(bytes: &[u8], meta: &std::fs::Metadata) -> FileVersion {
    FileVersion {
        hash: blake3::hash(bytes).to_hex().to_string(),
        size: bytes.len() as u64,
        modified_ms: meta.modified().map(millis).unwrap_or(0),
    }
}

/// Время файловой системы → миллисекунды UTC.
fn millis(time: std::time::SystemTime) -> i64 {
    match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(after) => after.as_millis() as i64,
        Err(before) => -(before.duration().as_millis() as i64),
    }
}

/// Ошибка ОС → ошибка ядра без подробностей (в ошибках для интерфейса нет
/// путей и текста ОС, инварианты 2, 3).
fn io_error(error: std::io::Error) -> CoreError {
    use std::io::ErrorKind;
    CoreError::new(match error.kind() {
        ErrorKind::NotFound | ErrorKind::NotADirectory => ErrorCode::NotFound,
        ErrorKind::AlreadyExists | ErrorKind::DirectoryNotEmpty => ErrorCode::AlreadyExists,
        _ => ErrorCode::IoFailed,
    })
}

fn unavailable() -> CoreError {
    CoreError::new(ErrorCode::VaultUnavailable)
}

#[cfg(test)]
mod tests;
