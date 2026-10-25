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
pub(crate) mod fsops;
mod local_time;
mod own_writes;
mod settings;
mod text;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock};

use crate::commands::{
    CoreError, CoreResult, EntryKind, ErrorCode, FileVersion, NameCheck, NameProblem,
    NoteReadResponse, NoteWriteResponse, ReadOnlyReason, Settings, TrashOutcome, TreeEntry,
    VaultPath, Workspace,
};

pub use contour::{SERVICE_DIR, check_component};
pub use settings::{SETTINGS_FORMAT_VERSION, SettingsFile};

use contour::Contour;

/// Временные файлы атомарной записи — `.rhizomind/tmp/`.
const TMP_DIR: &str = "tmp";
/// Настройки хранилища — `.rhizomind/settings.json`.
const SETTINGS_FILE: &str = "settings.json";
/// Раскладка окна — `.rhizomind/workspace.json`.
const WORKSPACE_FILE: &str = "workspace.json";
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

/// Почему корзина ОС не приняла элемент.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrashFailure {
    /// На томе нет корзины (флешка, сетевая папка) или корзина отказала.
    NoTrash,
    /// Корзина есть, но не приняла элемент.
    Failed,
}

/// Удаление в корзину ОС по абсолютному пути (подменяется в тестах).
pub type TrashBackend = Box<dyn Fn(&Path) -> Result<(), TrashFailure> + Send + Sync + 'static>;

/// Открытое хранилище: корень и операции над файлами внутри контура.
///
/// Все методы принимают пути [`VaultPath`] от интерфейса как есть и сами
/// проверяют их компоненты; пределы длины уже проверены слоем команд.
/// Методы блокируют поток (диск, корзина) — команды вызывают их вне
/// главного потока.
pub struct Vault {
    contour: Contour,
    /// `.rhizomind/` — канонический путь, проверен при открытии.
    service: PathBuf,
    /// Свои записи для наблюдателя за папкой.
    own_writes: own_writes::OwnWrites,
    /// Подменённая корзина (тесты); `None` — корзина ОС.
    trash_backend: RwLock<Option<TrashBackend>>,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("root", &self.contour.root())
            .finish_non_exhaustive()
    }
}

impl Vault {
    /// Создаёт папку нового хранилища `parent/name` (не существующую).
    /// Имя уже проверено правилами имён и [`check_component`]; возвращает
    /// путь к созданной папке. `.rhizomind/` создаёт [`Vault::open`].
    pub fn create(parent: &Path, name: &str) -> CoreResult<PathBuf> {
        check_component(name)?;
        let parent = dunce::canonicalize(parent).map_err(|_| unavailable())?;
        let target = parent.join(name);
        std::fs::create_dir(&target).map_err(io_error)?;
        dunce::canonicalize(&target).map_err(io_error)
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
        let vault = Self {
            contour,
            service,
            own_writes: own_writes::OwnWrites::default(),
            trash_backend: RwLock::new(None),
        };
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
    /// Символические ссылки и точки соединения не показываются и не
    /// обходятся; имена, к которым нельзя обратиться командой (не UTF-8, не
    /// проходят [`check_component`]), тоже не показываются.
    pub fn tree(&self) -> CoreResult<Vec<TreeEntry>> {
        let mut entries = Vec::new();
        self.walk(self.root(), &mut Vec::new(), &mut |path, kind, _| {
            entries.push(TreeEntry { path, kind });
        })?;
        Ok(entries)
    }

    /// Все заметки хранилища с датой создания — для модулей `links` и
    /// `switcher`. «Самая старая» из заметок с одним именем — с наименьшей
    /// `created_ms`; при равной дате — первая по пути в естественном
    /// алфавитном порядке (С3.4). ФС без даты создания — дата изменения.
    pub fn notes(&self) -> CoreResult<Vec<NoteRef>> {
        let mut notes = Vec::new();
        self.walk(self.root(), &mut Vec::new(), &mut |path, kind, entry| {
            if kind == EntryKind::Note {
                let created_ms = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.created().or_else(|_| m.modified()).ok())
                    .map(millis)
                    .unwrap_or(0);
                notes.push(NoteRef { path, created_ms });
            }
        })?;
        Ok(notes)
    }

    /// Обход видимых элементов в глубину, без перехода по ссылкам.
    fn walk(
        &self,
        dir: &Path,
        prefix: &mut Vec<String>,
        visit: &mut dyn FnMut(VaultPath, EntryKind, &std::fs::DirEntry),
    ) -> CoreResult<()> {
        if prefix.len() >= crate::commands::limits::MAX_PATH_COMPONENTS {
            return Ok(());
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            // Папку удалили во время обхода или нет прав — пропускается.
            return if prefix.is_empty() {
                Err(unavailable())
            } else {
                Ok(())
            };
        };
        for entry in entries.flatten() {
            let Some((name, file_type)) = fsops::visible(&entry) else {
                continue;
            };
            prefix.push(name);
            let path = VaultPath::from_components(prefix.iter().cloned());
            if file_type.is_dir() {
                visit(path, EntryKind::Folder, &entry);
                self.walk(&entry.path(), prefix, visit)?;
            } else {
                let name = prefix.last().map(String::as_str).unwrap_or_default();
                let kind = if crate::names::is_note(name) {
                    EntryKind::Note
                } else {
                    EntryKind::Attachment
                };
                visit(path, kind, &entry);
            }
            prefix.pop();
        }
        Ok(())
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

    /// Регистрирует получателя своих записей — наблюдатель за папкой
    /// (задача 1.4). Получатель вызывается до замены файла на диске.
    pub fn set_own_write_hook(&self, hook: OwnWriteHook) {
        self.own_writes.set_hook(hook);
    }

    /// Своя ли это недавняя запись ядра (путь, хэш и размер новой версии):
    /// событие о таком файле — не внешнее изменение.
    pub fn is_own_write(&self, path: &VaultPath, hash: &str, size: u64) -> bool {
        self.own_writes.contains(path, hash, size)
    }

    /// Записывает заметку атомарно по правилу «байт в байт»
    /// (`commands::note`): BOM и переносы — из файла на диске; текст после
    /// приведения совпадает с диском — файл не пишется. Содержимое на диске
    /// (хэш и размер) не равно `expected` — изменения снаружи не затираются:
    /// версия с диска сохраняется копией рядом, текст из редактора — в
    /// заметку (`conflict_saved`, С3.6). Только дата изменения на диске
    /// другая — не конфликт. Файла нет — `not_found`. Дата создания
    /// сохраняется (см. описание модуля).
    pub fn write_note(
        &self,
        path: &VaultPath,
        text: &str,
        expected: &FileVersion,
    ) -> CoreResult<NoteWriteResponse> {
        let abs = self.contour.resolve(path)?;
        require_note_name(path)?;
        let (disk, disk_version) = self.read_existing_note(&abs)?;
        let decoded = text::decode(&disk);
        let same_content = disk_version.hash == expected.hash && disk_version.size == expected.size;
        if decoded.invalid_utf8 && same_content {
            return Err(CoreError::new(ErrorCode::ReadOnly));
        }
        if !decoded.invalid_utf8 && decoded.text == text::normalize(text) {
            return Ok(NoteWriteResponse::Saved {
                version: disk_version,
            });
        }
        let bytes = text::Format::of(&disk).encode(text);
        if same_content {
            let version = self.replace_note(path, &abs, &bytes)?;
            return Ok(NoteWriteResponse::Saved { version });
        }
        // Сначала копия версии с диска, потом замена: версия с диска не
        // теряется ни при каком сбое.
        let copy_path = self.save_conflict_copy(path, &abs, &disk, &disk_version)?;
        let version = self.replace_note(path, &abs, &bytes)?;
        Ok(NoteWriteResponse::ConflictSaved { version, copy_path })
    }

    /// Атомарно заменяет существующую заметку; своя запись регистрируется до
    /// замены. Возвращает версию нового файла.
    fn replace_note(&self, path: &VaultPath, abs: &Path, bytes: &[u8]) -> CoreResult<FileVersion> {
        let hash = blake3::hash(bytes).to_hex().to_string();
        let temp = atomic::write_temp(&self.tmp_dir(), bytes).map_err(io_error)?;
        self.own_writes.register(path, &hash, bytes.len() as u64);
        atomic::replace(temp, abs).map_err(io_error)?;
        self.version_after_write(abs, hash, bytes.len() as u64)
    }

    /// Версия только что записанного файла: хэш и размер известны, дата —
    /// с диска (дескриптор проверен по контуру).
    fn version_after_write(&self, abs: &Path, hash: String, size: u64) -> CoreResult<FileVersion> {
        let file = File::open(abs).map_err(io_error)?;
        self.contour.check_opened(&file, abs, false)?;
        let meta = file.metadata().map_err(io_error)?;
        Ok(FileVersion {
            hash,
            size,
            modified_ms: meta.modified().map(millis).unwrap_or(0),
        })
    }

    /// Копия версии с диска рядом с заметкой: «Имя (внешняя версия
    /// ГГГГ-ММ-ДД ЧЧ-ММ).md» по местному времени изменения файла; имя занято
    /// (повтор в ту же минуту) — с номером: «… ЧЧ-ММ) 2.md». Файл появляется
    /// без затирания существующих (`persist_noclobber`).
    fn save_conflict_copy(
        &self,
        path: &VaultPath,
        abs: &Path,
        disk: &[u8],
        disk_version: &FileVersion,
    ) -> CoreResult<VaultPath> {
        let name = path.file_name().ok_or_else(contour::invalid_path)?;
        let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
        let modified = std::time::UNIX_EPOCH
            + std::time::Duration::from_millis(disk_version.modified_ms.max(0) as u64);
        let stamp = local_time::minute_stamp(modified);
        let dir = abs.parent().ok_or_else(contour::invalid_path)?;
        let parent: Vec<String> = path.components()[..path.components().len() - 1].to_vec();
        let hash = blake3::hash(disk).to_hex().to_string();
        for attempt in 1..=1000u32 {
            let suffix = if attempt == 1 {
                format!(" (внешняя версия {stamp})")
            } else {
                format!(" (внешняя версия {stamp}) {attempt}")
            };
            let copy_name = fit_name(stem, &suffix, extension);
            if self.sibling_taken(dir, &copy_name, None)? {
                continue;
            }
            let copy_path =
                VaultPath::from_components(parent.iter().cloned().chain([copy_name.clone()]));
            let temp = atomic::write_temp(&self.tmp_dir(), disk).map_err(io_error)?;
            self.own_writes
                .register(&copy_path, &hash, disk.len() as u64);
            match atomic::create_new(temp, &dir.join(&copy_name), Some(abs)) {
                Ok(()) => return Ok(copy_path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io_error(e)),
            }
        }
        Err(CoreError::new(ErrorCode::IoFailed))
    }

    /// Есть ли в папке `dir` элемент с тем же ключом имени, что `name`
    /// (кроме элемента с именем `except` — переименовываемого).
    fn sibling_taken(&self, dir: &Path, name: &str, except: Option<&str>) -> CoreResult<bool> {
        let key = crate::names::key(name);
        for entry in std::fs::read_dir(dir).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let Some(existing) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if except == Some(existing.as_str()) {
                continue;
            }
            if crate::names::key(&existing) == key {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Создаёт пустую заметку `name` (полное имя с `.md`; UTF-8 без BOM,
    /// переносы LF — пустой файл) в папке `dir`. Правила имён проверяются
    /// и здесь (защита в глубину, повторное ревью 0.4.1, п. 2).
    pub fn create_note(&self, dir: &VaultPath, name: &str) -> CoreResult<VaultPath> {
        check_name(name)?;
        if !crate::names::is_note(name) {
            return Err(CoreError::invalid_name(NameProblem::NoteExtension));
        }
        let (dir_abs, path) = self.prepare_new_entry(dir, name)?;
        let empty = blake3::hash(b"").to_hex().to_string();
        self.own_writes.register(&path, &empty, 0);
        File::create_new(dir_abs.join(name)).map_err(io_error)?;
        Ok(path)
    }

    /// Создаёт папку `name` в папке `dir`.
    pub fn create_folder(&self, dir: &VaultPath, name: &str) -> CoreResult<VaultPath> {
        check_name(name)?;
        let (dir_abs, path) = self.prepare_new_entry(dir, name)?;
        std::fs::create_dir(dir_abs.join(name)).map_err(io_error)?;
        Ok(path)
    }

    /// Папка существует, проверена по дескриптору, имени `name` в ней нет
    /// (по ключу имени). Возвращает путь папки на диске и путь нового
    /// элемента.
    fn prepare_new_entry(&self, dir: &VaultPath, name: &str) -> CoreResult<(PathBuf, VaultPath)> {
        let path = child(dir, name);
        self.contour.resolve(&path)?;
        let dir_abs = self.existing_dir(dir)?;
        if self.sibling_taken(&dir_abs, name, None)? {
            return Err(CoreError::new(ErrorCode::AlreadyExists));
        }
        Ok((dir_abs, path))
    }

    /// Существующая папка хранилища (не файл), итоговый путь проверен по
    /// дескриптору. Нет или это файл — `not_found`.
    fn existing_dir(&self, dir: &VaultPath) -> CoreResult<PathBuf> {
        let abs = self.contour.resolve(dir)?;
        let meta = std::fs::symlink_metadata(&abs).map_err(io_error)?;
        if !meta.is_dir() {
            return Err(CoreError::new(ErrorCode::NotFound));
        }
        let handle = contour::open_dir(&abs).map_err(io_error)?;
        self.contour.check_opened(&handle, &abs, false)?;
        Ok(abs)
    }

    /// Существующий элемент хранилища (не корень), итоговый путь проверен
    /// по дескриптору (у папки — дескриптор папки).
    fn existing_entry(&self, path: &VaultPath) -> CoreResult<(PathBuf, std::fs::Metadata)> {
        require_not_root(path)?;
        let abs = self.contour.resolve(path)?;
        let meta = std::fs::symlink_metadata(&abs).map_err(io_error)?;
        let handle = if meta.is_dir() {
            contour::open_dir(&abs)
        } else {
            File::open(&abs)
        }
        .map_err(io_error)?;
        self.contour.check_opened(&handle, &abs, false)?;
        Ok((abs, meta))
    }

    /// Совпадение имени с соседями в папке `dir` по ключу имени
    /// (`names::key`); правила самого имени проверяются и здесь.
    /// `renaming` — переименовываемый элемент: он не совпадение сам с собой,
    /// поэтому смена только регистра разрешена.
    pub fn check_new_name(
        &self,
        dir: &VaultPath,
        name: &str,
        renaming: Option<&VaultPath>,
    ) -> CoreResult<NameCheck> {
        let dir_abs = self.existing_dir(dir)?;
        if let Some(problem) = crate::names::check(name) {
            return Ok(NameCheck::rejected(problem));
        }
        let except = renaming
            .filter(|r| r.components().len() == dir.components().len() + 1)
            .filter(|r| same_keys(&r.components()[..dir.components().len()], dir.components()))
            .and_then(VaultPath::file_name);
        if self.sibling_taken(&dir_abs, name, except)? {
            return Ok(NameCheck::rejected(NameProblem::Duplicate));
        }
        Ok(NameCheck::Ok)
    }

    /// Переименовывает файл или папку; возвращает новый путь. Существующий
    /// элемент не затирается (`already_exists`). Смена только регистра
    /// разрешена; на Windows — через промежуточное скрытое имя в той же
    /// папке.
    pub fn rename(&self, path: &VaultPath, new_name: &str) -> CoreResult<VaultPath> {
        let (abs, _) = self.existing_entry(path)?;
        check_name(new_name)?;
        let old_name = path.file_name().ok_or_else(contour::invalid_path)?;
        if let Some(problem) = crate::names::check_rename_keeps_kind(old_name, new_name) {
            return Err(CoreError::invalid_name(problem));
        }
        let new_path = child(
            &VaultPath::from_components(
                path.components()[..path.components().len() - 1]
                    .iter()
                    .cloned(),
            ),
            new_name,
        );
        self.contour.resolve(&new_path)?;
        if new_name == old_name {
            return Ok(path.clone());
        }
        let parent = abs.parent().ok_or_else(contour::invalid_path)?;
        if self.sibling_taken(parent, new_name, Some(old_name))? {
            return Err(CoreError::new(ErrorCode::AlreadyExists));
        }
        let target = parent.join(new_name);
        let case_only = crate::names::key(old_name) == crate::names::key(new_name);
        if case_only && cfg!(windows) {
            let step = parent.join(format!(
                ".{}.rhizomind-rename",
                blake3::hash(new_name.as_bytes()).to_hex()
            ));
            fsops::rename_noreplace(&abs, &step).map_err(io_error)?;
            if let Err(error) = fsops::rename_noreplace(&step, &target) {
                let _ = fsops::rename_noreplace(&step, &abs);
                return Err(io_error(error));
            }
        } else {
            fsops::rename_noreplace(&abs, &target).map_err(io_error)?;
        }
        Ok(new_path)
    }

    /// Перемещает файл или папку в `new_dir`; возвращает новый путь.
    /// Существующий элемент не затирается; папку нельзя переместить в себя
    /// или в свою вложенную папку (`invalid_path`).
    pub fn move_entry(&self, path: &VaultPath, new_dir: &VaultPath) -> CoreResult<VaultPath> {
        // Сначала контур обоих путей, потом их наличие на диске.
        require_not_root(path)?;
        self.contour.resolve(path)?;
        self.contour.resolve(new_dir)?;
        let (abs, _) = self.existing_entry(path)?;
        let dir_abs = self.existing_dir(new_dir)?;
        let n = path.components().len();
        if new_dir.components().len() >= n
            && same_keys(&new_dir.components()[..n], path.components())
        {
            return Err(contour::invalid_path());
        }
        let name = path.file_name().ok_or_else(contour::invalid_path)?;
        if new_dir.components().len() == n - 1
            && same_keys(new_dir.components(), &path.components()[..n - 1])
        {
            return Ok(path.clone());
        }
        if self.sibling_taken(&dir_abs, name, None)? {
            return Err(CoreError::new(ErrorCode::AlreadyExists));
        }
        fsops::rename_noreplace(&abs, &dir_abs.join(name)).map_err(io_error)?;
        Ok(child(new_dir, name))
    }

    /// Подменяет корзину ОС (тесты; на настоящем ядре — `fsops::os_trash`).
    pub fn set_trash_backend(&self, backend: TrashBackend) {
        *self
            .trash_backend
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(backend);
    }

    /// Удаляет в корзину ОС (в отдельном потоке). На томе без корзины
    /// спрашивает `confirm_permanent` (окно подтверждения ядра, задача 1.7)
    /// и удаляет безвозвратно только при согласии — после повторной
    /// проверки пути по контуру (между вопросом и удалением могло пройти
    /// время).
    pub fn trash(
        &self,
        path: &VaultPath,
        confirm_permanent: &dyn Fn(&VaultPath) -> bool,
    ) -> CoreResult<TrashOutcome> {
        let (abs, _) = self.existing_entry(path)?;
        let result = match self
            .trash_backend
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            Some(backend) => backend(&abs),
            None => fsops::os_trash(&abs),
        };
        match result {
            Ok(()) => Ok(TrashOutcome::Trashed),
            Err(TrashFailure::NoTrash | TrashFailure::Failed) => {
                if !confirm_permanent(path) {
                    return Ok(TrashOutcome::Cancelled);
                }
                let (abs, meta) = self.existing_entry(path)?;
                if meta.is_dir() {
                    std::fs::remove_dir_all(&abs)
                } else {
                    std::fs::remove_file(&abs)
                }
                .map_err(io_error)?;
                Ok(TrashOutcome::DeletedPermanently)
            }
        }
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
    pub fn write_settings(&self, settings: &Settings) -> CoreResult<()> {
        let mut file = self.read_settings_file()?;
        file.update(settings);
        let bytes =
            serde_json::to_vec_pretty(&file).map_err(|_| CoreError::new(ErrorCode::IoFailed))?;
        self.write_service_file(SETTINGS_FILE, &bytes)
    }

    /// Раскладка окна из `.rhizomind/workspace.json`; файла нет — `None`.
    /// Не JSON-объект — как будто раскладки нет. Модель раскладки v1 и её
    /// версия — внутри объекта (ADR 0003, интерфейс); ядро хранит как есть.
    pub fn read_workspace(&self) -> CoreResult<Option<Workspace>> {
        Ok(self
            .read_service_file(WORKSPACE_FILE)?
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| match value {
                serde_json::Value::Object(map) => Some(Workspace(map)),
                _ => None,
            }))
    }

    /// Записывает раскладку окна атомарно.
    pub fn write_workspace(&self, workspace: &Workspace) -> CoreResult<()> {
        let bytes = serde_json::to_vec_pretty(workspace)
            .map_err(|_| CoreError::new(ErrorCode::IoFailed))?;
        self.write_service_file(WORKSPACE_FILE, &bytes)
    }

    /// Атомарно пишет служебный файл `.rhizomind/<name>`; файл-ссылка не
    /// заменяется (`outside_vault`).
    fn write_service_file(&self, name: &str, bytes: &[u8]) -> CoreResult<()> {
        let path = self.service.join(name);
        let exists = match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
                return Err(contour::outside_vault());
            }
            Ok(_) => true,
            Err(_) => false,
        };
        let temp = atomic::write_temp(&self.tmp_dir(), bytes).map_err(io_error)?;
        if exists {
            return atomic::replace(temp, &path).map_err(io_error);
        }
        match atomic::create_new(temp, &path, None) {
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let temp = atomic::write_temp(&self.tmp_dir(), bytes).map_err(io_error)?;
                atomic::replace(temp, &path).map_err(io_error)
            }
            other => other.map_err(io_error),
        }
    }
}

/// Путь `dir/name`.
fn child(dir: &VaultPath, name: &str) -> VaultPath {
    VaultPath::from_components(dir.components().iter().cloned().chain([name.to_owned()]))
}

/// Имя `stem + suffix + .extension`, укороченное по основе до предела NTFS
/// (255 единиц UTF-16); точка и пробел в конце основы убираются.
fn fit_name(stem: &str, suffix: &str, extension: &str) -> String {
    let tail = if extension.is_empty() {
        suffix.to_owned()
    } else {
        format!("{suffix}.{extension}")
    };
    let budget =
        crate::commands::limits::MAX_COMPONENT_UTF16.saturating_sub(tail.encode_utf16().count());
    let mut base = String::new();
    let mut used = 0;
    for c in stem.chars() {
        if used + c.len_utf16() > budget {
            break;
        }
        used += c.len_utf16();
        base.push(c);
    }
    format!("{}{tail}", base.trim_end_matches(['.', ' ']))
}

/// Правила имён для нового имени: `names::check` (с причиной для
/// интерфейса), затем правила контура и служебная папка (`invalid_path`).
fn check_name(name: &str) -> CoreResult<()> {
    if let Some(problem) = crate::names::check(name) {
        return Err(CoreError::invalid_name(problem));
    }
    check_component(name)?;
    if contour::is_service_dir(name) {
        return Err(contour::invalid_path());
    }
    Ok(())
}

/// Пути совпадают по ключам имён компонентов.
fn same_keys(a: &[String], b: &[String]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| crate::names::key(x) == crate::names::key(y))
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
