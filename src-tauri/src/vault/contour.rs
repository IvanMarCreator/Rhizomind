//! Контур хранилища: проверка путей от интерфейса и итогового пути открытого
//! файла (инвариант 2, ADR 0009, исследование крейтов — «Контур хранилища»).
//!
//! Порядок для каждого пути от интерфейса:
//! 1. каждый компонент — [`check_component`] (`..`, `.`, `:`, разделители,
//!    зарезервированные имена Windows, точка или пробел в конце, короткие
//!    имена 8.3);
//! 2. первый компонент — не служебная папка `.rhizomind` по ключу имени
//!    ([`is_service_dir`]);
//! 3. корень канонизирован один раз при открытии (`dunce::canonicalize`);
//! 4. каждый промежуточный компонент и сам элемент — `symlink_metadata`:
//!    символическая ссылка или точка соединения NTFS → `outside_vault`;
//! 5. итоговый путь начинается с корня;
//! 6. после открытия файла или папки — итоговый путь **по дескриптору**
//!    ([`Contour::check_opened`]): `GetFinalPathNameByHandleW` на Windows,
//!    `/proc/self/fd` на Linux. Он ловит подмену компонента ссылкой между
//!    проверкой и открытием и короткие имена, которые пропустил шаг 1.

use std::fs::File;
use std::path::{Component, Path, PathBuf};

use crate::commands::{CoreError, CoreResult, ErrorCode, VaultPath};

/// Имя служебной папки хранилища.
pub const SERVICE_DIR: &str = ".rhizomind";

/// Проверка одного компонента пути от интерфейса. Ошибка — `invalid_path`.
///
/// Отклоняются: пустой, `.` и `..`; разделители `/` и `\` (а с ними `\\?\`,
/// `\\.\` и абсолютные пути); `:` (буквы дисков и потоки NTFS `имя:поток`);
/// NUL, управляющие символы и прочие запрещённые в Windows `* ? " < > |`;
/// зарезервированные имена Windows, в том числе с расширением (`nul.md`);
/// точка или пробел в конце; имена вида короткого имени 8.3 (`RHIZOM~1`,
/// `ПРОЕКТ~2.MD`) — через них NTFS открыла бы элемент под другим именем, в
/// том числе служебную папку.
pub fn check_component(component: &str) -> CoreResult<()> {
    let forbidden = |c: char| {
        c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
    };
    let ok = !component.is_empty()
        && component != "."
        && component != ".."
        && !component.chars().any(forbidden)
        && !component.ends_with('.')
        && !component.ends_with(' ')
        && !is_reserved_device(component)
        && !looks_like_short_name(component);
    if ok { Ok(()) } else { Err(invalid_path()) }
}

/// Зарезервированное имя устройства Windows: основа до первой точки (без
/// пробелов в конце) — `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`,
/// `COM1`–`COM9`, `LPT1`–`LPT9` и `COM¹`–`COM³`, `LPT¹`–`LPT³`.
fn is_reserved_device(component: &str) -> bool {
    let base = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end_matches(' ');
    let upper = base.to_uppercase();
    matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        upper.strip_prefix(prefix).is_some_and(|rest| {
            matches!(
                rest,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

/// Имя по форме короткого имени 8.3, которое NTFS создаёт сама: основа до
/// 8 символов с `~` и цифрами в конце (`RHIZOM~1`, `AB~12`), расширение до
/// 3 символов. Длинные имена с тильдой (`Версия~2 итог.md`) короткими быть
/// не могут и проходят.
fn looks_like_short_name(component: &str) -> bool {
    let (base, ext) = match component.rsplit_once('.') {
        Some((base, ext)) if !base.is_empty() => (base, Some(ext)),
        _ => (component, None),
    };
    if base.chars().count() > 8 || ext.is_some_and(|e| e.chars().count() > 3 || e.contains('~')) {
        return false;
    }
    let Some((head, digits)) = base.rsplit_once('~') else {
        return false;
    };
    !head.is_empty()
        && !head.contains('.')
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
}

/// Служебная папка по ключу имени: `.rhizomind`, `.Rhizomind`, `.RHIZOMIND`.
pub fn is_service_dir(component: &str) -> bool {
    crate::names::key(component) == crate::names::key(SERVICE_DIR)
}

pub(crate) fn invalid_path() -> CoreError {
    CoreError::new(ErrorCode::InvalidPath)
}

pub(crate) fn outside_vault() -> CoreError {
    CoreError::new(ErrorCode::OutsideVault)
}

/// Канонический корень хранилища и проверка путей внутри него.
#[derive(Debug, Clone)]
pub struct Contour {
    root: PathBuf,
}

impl Contour {
    /// Корень уже канонизирован (`dunce::canonicalize`).
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Корень хранилища.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Абсолютный путь элемента хранилища после шагов 1–5 описания модуля.
    /// Сам элемент может не существовать (создание); существующий элемент и
    /// все промежуточные компоненты — не ссылки.
    pub fn resolve(&self, path: &VaultPath) -> CoreResult<PathBuf> {
        path.check()?;
        if path
            .components()
            .first()
            .is_some_and(|first| is_service_dir(first))
        {
            return Err(invalid_path());
        }
        let mut current = self.root.clone();
        for component in path.components() {
            current.push(component);
            match std::fs::symlink_metadata(&current) {
                Ok(meta) if meta.file_type().is_symlink() => return Err(outside_vault()),
                Ok(_) => {}
                // Дальше по пути ничего нет: ссылкой он стать уже не может —
                // это проверит дескриптор после открытия.
                Err(_) => break,
            }
        }
        if !current.starts_with(&self.root) {
            return Err(outside_vault());
        }
        Ok(current)
    }

    /// Шаг 6: итоговый путь открытого файла или папки лежит в корне и не в
    /// служебной папке. `allow_service` — для своих служебных файлов ядра.
    pub fn check_opened(
        &self,
        file: &File,
        fallback: &Path,
        allow_service: bool,
    ) -> CoreResult<()> {
        let real = final_path(file).or_else(|| {
            std::fs::canonicalize(fallback)
                .ok()
                .map(|p| dunce::simplified(&p).to_path_buf())
        });
        let Some(real) = real else {
            return Err(outside_vault());
        };
        self.check_real_path(&real, allow_service)
    }

    /// Проверка уже разрешённого (итогового) пути.
    pub fn check_real_path(&self, real: &Path, allow_service: bool) -> CoreResult<()> {
        let rest = strip_root(&self.root, real).ok_or_else(outside_vault)?;
        let mut parts = rest.components();
        match parts.next() {
            None => Ok(()),
            Some(Component::Normal(first)) => {
                let service = first.to_str().is_none_or(is_service_dir);
                if service && !allow_service {
                    Err(outside_vault())
                } else {
                    Ok(())
                }
            }
            Some(_) => Err(outside_vault()),
        }
    }
}

/// Остаток пути после корня. На Windows компоненты сравниваются без учёта
/// регистра (NTFS), на других ОС — как есть.
fn strip_root<'a>(root: &Path, real: &'a Path) -> Option<&'a Path> {
    if cfg!(windows) {
        let mut real_parts = real.components();
        for root_part in root.components() {
            let real_part = real_parts.next()?;
            let same = root_part.as_os_str().to_string_lossy().to_lowercase()
                == real_part.as_os_str().to_string_lossy().to_lowercase();
            if !same {
                return None;
            }
        }
        Some(real_parts.as_path())
    } else {
        real.strip_prefix(root).ok()
    }
}

/// Итоговый путь открытого файла по дескриптору; `None` — ОС не сообщает.
#[cfg(windows)]
pub fn final_path(file: &File) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
    };

    let handle = file.as_raw_handle();
    let mut buffer = vec![0u16; 512];
    loop {
        // SAFETY: дескриптор открыт (живёт, пока жив `file`), буфер нужной длины.
        let len = unsafe {
            GetFinalPathNameByHandleW(
                handle,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        } as usize;
        if len == 0 {
            return None;
        }
        if len < buffer.len() {
            let path = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len]));
            return Some(dunce::simplified(&path).to_path_buf());
        }
        buffer.resize(len + 1, 0);
    }
}

/// Итоговый путь открытого файла по дескриптору; `None` — ОС не сообщает.
#[cfg(target_os = "linux")]
pub fn final_path(file: &File) -> Option<PathBuf> {
    use std::os::fd::AsRawFd;
    std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).ok()
}

/// Итоговый путь открытого файла по дескриптору; на этой ОС — проверка по
/// пути (`canonicalize`), см. `docs/SECURITY.md`.
#[cfg(not(any(windows, target_os = "linux")))]
pub fn final_path(_file: &File) -> Option<PathBuf> {
    None
}

/// Открывает папку для проверки итогового пути (на Windows папке нужен
/// `FILE_FLAG_BACKUP_SEMANTICS`).
pub fn open_dir(path: &Path) -> std::io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_keeps_components_after_a_missing_folder() {
        let dir = tempfile::tempdir().expect("папка");
        let root = dunce::canonicalize(dir.path()).expect("корень");
        let contour = Contour::new(root.clone());
        let path = VaultPath::from_components(["Нет", "Глубже", "x.md"]);
        assert_eq!(
            contour.resolve(&path),
            Ok(root.join("Нет").join("Глубже").join("x.md"))
        );
    }

    #[test]
    fn opened_file_outside_root_or_in_service_dir_is_rejected() {
        let outer = tempfile::tempdir().expect("папка");
        let root = outer.path().join("vault");
        std::fs::create_dir_all(root.join(SERVICE_DIR)).expect("папки");
        let root = dunce::canonicalize(&root).expect("корень");
        let contour = Contour::new(root.clone());

        let outside = outer.path().join("secret.md");
        std::fs::write(&outside, "секрет").expect("файл");
        let file = File::open(&outside).expect("открыт");
        assert_eq!(
            contour.check_opened(&file, &outside, false),
            Err(outside_vault())
        );

        let service = root.join(SERVICE_DIR).join("settings.json");
        std::fs::write(&service, "{}").expect("файл");
        let file = File::open(&service).expect("открыт");
        assert_eq!(
            contour.check_opened(&file, &service, false),
            Err(outside_vault())
        );
        assert_eq!(contour.check_opened(&file, &service, true), Ok(()));

        let note = root.join("Заметка.md");
        std::fs::write(&note, "текст").expect("файл");
        let file = File::open(&note).expect("открыт");
        assert_eq!(contour.check_opened(&file, &note, false), Ok(()));
    }
}
