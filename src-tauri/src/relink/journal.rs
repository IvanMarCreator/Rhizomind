//! Журнал незавершённого обновления ссылок `.rhizomind/link-update.json`
//! (задача 0.2-4.5; С2-4, С2-5.4 шаги 3 и 5, «Отката нет», Р8, план 0.2, Т5).
//!
//! Что в журнале: версия формата, переименования «откуда → куда», заметка,
//! которая записывается сейчас (путь и хэш нового содержимого), и ещё не
//! переписанные заметки с тройками (`old_target`, `new_target`,
//! `points_to`) — без смещений: при доделке ссылки находятся заново. Текста
//! заметок в журнале нет.
//!
//! Пишется только через [`Vault::write_service_file`]: атомарно, внутри
//! проверенной по дескриптору служебной папки; ссылка вместо журнала не
//! заменяется. Читается с собственным пределом [`MAX_JOURNAL_BYTES`] (перенос
//! папки на десятки тысяч заметок больше общих 4 МБ служебных файлов).
//!
//! Журнал на диске — **недоверенные данные** (его могли поправить руками или
//! подменить): разбирается в типы без `serde_json::Value`, лишние поля и
//! другая версия — журнал отбрасывается; пути проверяются правилами контура
//! перед использованием, а запись идёт только через контур хранилища.

use serde::{Deserialize, Serialize};

use super::{NotePlan, PlannedLink};
use crate::commands::{CoreError, CoreResult, ErrorCode, PathRename, VaultPath};
use crate::vault::{ServiceRead, Vault};

/// Имя файла журнала в `.rhizomind/`.
pub const JOURNAL_FILE: &str = "link-update.json";
/// Версия формата журнала.
pub const FORMAT_VERSION: u32 = 1;
/// Журнал больше этого не пишется и не читается.
pub const MAX_JOURNAL_BYTES: u64 = 32 * 1024 * 1024;

/// Заметка, запись которой начата: если при доделке на диске ровно это
/// содержимое — заметка уже переписана (падение между заменой файла и
/// следующей отметкой).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Writing {
    pub path: VaultPath,
    pub hash: String,
}

/// Прочитанный журнал.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journal {
    pub renames: Vec<PathRename>,
    pub writing: Option<Writing>,
    pub notes: Vec<NotePlan>,
}

/// Результат чтения журнала.
#[derive(Debug)]
pub enum Read {
    /// Журнала нет — доделывать нечего.
    Missing,
    /// Журнал есть, но не годится (не разбирается, другая версия, больше
    /// предела): доделать нельзя, журнал убирается.
    Unusable,
    /// Служебная папка недоступна или журнал — ссылка: не трогать.
    Refused,
    /// Журнал.
    Found(Journal),
}

#[derive(Serialize)]
struct LinkOut<'a> {
    old_target: &'a str,
    new_target: &'a str,
    points_to: &'a VaultPath,
}

#[derive(Serialize)]
struct NoteOut<'a> {
    path: &'a VaultPath,
    links: Vec<LinkOut<'a>>,
}

#[derive(Serialize)]
struct WritingOut<'a> {
    path: &'a VaultPath,
    hash: &'a str,
}

#[derive(Serialize)]
struct FileOut<'a> {
    format_version: u32,
    renames: &'a [PathRename],
    writing: Option<WritingOut<'a>>,
    notes: Vec<NoteOut<'a>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkIn {
    old_target: String,
    new_target: String,
    points_to: VaultPath,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteIn {
    path: VaultPath,
    links: Vec<LinkIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileIn {
    format_version: u32,
    renames: Vec<PathRename>,
    writing: Option<Writing>,
    notes: Vec<NoteIn>,
}

/// Пишет журнал атомарно: оставшиеся заметки `notes` и начатая запись
/// `writing` (путь и хэш). Больше [`MAX_JOURNAL_BYTES`] — ошибка (такой
/// журнал не прочитать при доделке).
pub fn write(
    vault: &Vault,
    renames: &[PathRename],
    writing: Option<(&VaultPath, &str)>,
    notes: &[NotePlan],
) -> CoreResult<()> {
    let file = FileOut {
        format_version: FORMAT_VERSION,
        renames,
        writing: writing.map(|(path, hash)| WritingOut { path, hash }),
        notes: notes
            .iter()
            .map(|note| NoteOut {
                path: &note.path,
                links: note
                    .links
                    .iter()
                    .map(|link| LinkOut {
                        old_target: &link.old_target,
                        new_target: &link.new_target,
                        points_to: &link.points_to,
                    })
                    .collect(),
            })
            .collect(),
    };
    let bytes = serde_json::to_vec(&file).map_err(|_| CoreError::new(ErrorCode::IoFailed))?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(CoreError::new(ErrorCode::IoFailed));
    }
    vault.write_service_file(JOURNAL_FILE, &bytes)
}

/// Читает журнал с пределом [`MAX_JOURNAL_BYTES`].
pub fn read(vault: &Vault) -> Read {
    match vault.read_service_file_limited(JOURNAL_FILE, MAX_JOURNAL_BYTES) {
        Err(_) => Read::Refused,
        Ok(ServiceRead::Missing) => Read::Missing,
        Ok(ServiceRead::TooLarge) => Read::Unusable,
        Ok(ServiceRead::Bytes(bytes)) => match parse(&bytes) {
            Some(journal) => Read::Found(journal),
            None => Read::Unusable,
        },
    }
}

/// Разбор байтов журнала; не тот формат — `None`.
fn parse(bytes: &[u8]) -> Option<Journal> {
    let file: FileIn = serde_json::from_slice(bytes).ok()?;
    if file.format_version != FORMAT_VERSION {
        return None;
    }
    Some(Journal {
        renames: file.renames,
        writing: file.writing,
        notes: file
            .notes
            .into_iter()
            .map(|note| NotePlan {
                path: note.path,
                links: note
                    .links
                    .into_iter()
                    .map(|link| PlannedLink {
                        old_target: link.old_target,
                        new_target: link.new_target,
                        points_to: link.points_to,
                    })
                    .collect(),
            })
            .collect(),
    })
}

/// Удаляет журнал (план выполнен или журнал не годится).
pub fn remove(vault: &Vault) -> CoreResult<()> {
    vault.remove_service_file(JOURNAL_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(path: &str) -> VaultPath {
        VaultPath::from_components(path.split('/'))
    }

    #[test]
    fn round_trip_keeps_renames_writing_and_triples() {
        let dir = tempfile::tempdir().expect("папка");
        let vault = Vault::open(dir.path()).expect("хранилище");
        let renames = [PathRename {
            from: p("Альфа"),
            to: p("Гамма"),
        }];
        let notes = [NotePlan {
            path: p("X.md"),
            links: vec![PlannedLink {
                old_target: "Альфа/Встреча".into(),
                new_target: "Гамма/Встреча".into(),
                points_to: p("Гамма/Встреча.md"),
            }],
        }];

        write(&vault, &renames, Some((&p("X.md"), "abc")), &notes).expect("запись");

        let Read::Found(journal) = read(&vault) else {
            panic!("журнал не прочитан");
        };
        assert_eq!(journal.renames, renames);
        assert_eq!(
            journal.writing,
            Some(Writing {
                path: p("X.md"),
                hash: "abc".into()
            })
        );
        assert_eq!(journal.notes, notes);
        let text = std::fs::read_to_string(dir.path().join(".rhizomind").join(JOURNAL_FILE))
            .expect("файл");
        assert!(text.starts_with(r#"{"format_version":1,"#), "{text}");
        remove(&vault).expect("удаление");
        assert!(matches!(read(&vault), Read::Missing));
    }
}
