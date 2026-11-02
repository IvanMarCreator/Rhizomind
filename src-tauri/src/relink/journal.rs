//! Журнал незавершённого обновления ссылок `.rhizomind/link-update.json`
//! (задача 0.2-4.5; С2-4, С2-5.4 шаги 3 и 5, «Отката нет», Р8, план 0.2, Т5).
//!
//! **Формат** — строки JSON, разделённые `\n`:
//! 1. заголовок: версия формата, переименования «откуда → куда» и все
//!    заметки плана с тройками (`old_target`, `new_target`, `points_to`) —
//!    без смещений, при доделке ссылки находятся заново; текста заметок нет;
//! 2. дальше — дописываемые отметки, по одной на строку:
//!    `{"start":{"note":i,"before":"…","after":"…"}}` — запись заметки `i`
//!    начата: хэши `blake3` прочитанного и нового содержимого (сброс на диск
//!    до замены файла); `{"done":i}` — заметка `i` готова (записана, менять
//!    нечего или её нет; без сброса на диск — потерянная отметка лишь
//!    заставит доделку сверить хэши).
//!
//! Объём на заметку — O(1): заголовок пишется один раз, отметки — около
//! 150 байт на заметку (замер `journal_volume_10000_notes`).
//!
//! **Где и как пишется.** Заголовок — атомарно через
//! [`Vault::write_service_file`] (временный файл в `.rhizomind/tmp/`, замена,
//! файл-ссылка не заменяется). Отметки — дописыванием в конец
//! ([`Vault::open_service_append`]): имя фиксированное
//! ([`ServiceFile::LinkUpdate`]), служебная папка и открытый файл проверены
//! по дескриптору до первой записи. Атомарность каждой отметки — при чтении:
//! строка после последнего `\n` (оборванная запись) отбрасывается.
//!
//! **Чтение** — с собственным пределом [`MAX_JOURNAL_BYTES`]. Журнал на диске
//! — недоверенные данные (его могли поправить руками или подменить):
//! разбирается в типы без `serde_json::Value`; лишние поля, другая версия,
//! испорченная строка в середине, отметка о заметке, которой нет в
//! заголовке, — журнал не годится целиком. Пути проверяет вызывающий
//! (`run::resume_on_open`), запись идёт только через контур хранилища.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{NotePlan, PlannedLink};
use crate::commands::{CoreError, CoreResult, ErrorCode, PathRename, VaultPath};
use crate::vault::{ServiceAppend, ServiceFile, ServiceRead, Vault};

/// Файл журнала.
const FILE: ServiceFile = ServiceFile::LinkUpdate;
/// Версия формата журнала.
pub const FORMAT_VERSION: u32 = 1;
/// Журнал больше этого не читается (и заголовок больше половины не пишется:
/// остаток — для отметок).
pub const MAX_JOURNAL_BYTES: u64 = 32 * 1024 * 1024;

/// Начатая запись заметки: хэши прочитанного и нового содержимого.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Started {
    pub before: String,
    pub after: String,
}

/// Прочитанный журнал.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journal {
    pub renames: Vec<PathRename>,
    /// Заметки плана из заголовка (номера отметок — номера в этом списке).
    pub notes: Vec<NotePlan>,
    /// Готовые заметки.
    pub done: HashSet<usize>,
    /// Начатые записи по номеру заметки (при повторах — несколько).
    pub started: HashMap<usize, Vec<Started>>,
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
struct HeaderOut<'a> {
    format_version: u32,
    renames: &'a [PathRename],
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
struct HeaderIn {
    format_version: u32,
    renames: Vec<PathRename>,
    notes: Vec<NoteIn>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartMark<S> {
    note: usize,
    before: S,
    after: S,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Mark<S> {
    Start(StartMark<S>),
    Done(usize),
}

/// Журнал, открытый для отметок.
#[derive(Debug)]
pub struct Writer {
    file: ServiceAppend,
}

impl Writer {
    /// Запись заметки `note` начата: хэши прочитанного и нового содержимого.
    /// Сбрасывается на диск до возврата — до замены файла заметки.
    pub fn started(&mut self, note: usize, before: &str, after: &str) -> CoreResult<()> {
        self.mark(
            &Mark::Start(StartMark {
                note,
                before,
                after,
            }),
            true,
        )
    }

    /// Заметка `note` готова. Без сброса на диск; ошибка не важна (доделка
    /// сверит хэши).
    pub fn done(&mut self, note: usize) {
        let _ = self.mark(&Mark::<&str>::Done(note), false);
    }

    fn mark(&mut self, mark: &Mark<&str>, sync: bool) -> CoreResult<()> {
        let mut line = serde_json::to_vec(mark).map_err(|_| CoreError::new(ErrorCode::IoFailed))?;
        line.push(b'\n');
        self.file.append(&line, sync)?;
        count(line.len());
        Ok(())
    }
}

/// Начинает журнал: заголовок с планом `notes` атомарно, затем файл
/// открывается для отметок. Заголовок больше половины
/// [`MAX_JOURNAL_BYTES`] — ошибка (журнал не прочитать при доделке).
pub fn start(vault: &Vault, renames: &[PathRename], notes: &[NotePlan]) -> CoreResult<Writer> {
    let header = HeaderOut {
        format_version: FORMAT_VERSION,
        renames,
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
    let mut bytes = serde_json::to_vec(&header).map_err(|_| CoreError::new(ErrorCode::IoFailed))?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_JOURNAL_BYTES / 2 {
        return Err(CoreError::new(ErrorCode::IoFailed));
    }
    vault.write_service_file(FILE.name(), &bytes)?;
    count(bytes.len());
    reopen(vault)
}

/// Открывает существующий журнал для отметок (доделка).
pub fn reopen(vault: &Vault) -> CoreResult<Writer> {
    Ok(Writer {
        file: vault.open_service_append(FILE)?,
    })
}

/// Читает журнал с пределом [`MAX_JOURNAL_BYTES`].
pub fn read(vault: &Vault) -> Read {
    match vault.read_service_file_limited(FILE.name(), MAX_JOURNAL_BYTES) {
        Err(_) => Read::Refused,
        Ok(ServiceRead::Missing) => Read::Missing,
        Ok(ServiceRead::TooLarge) => Read::Unusable,
        Ok(ServiceRead::Bytes(bytes)) => match parse(&bytes) {
            Some(journal) => Read::Found(journal),
            None => Read::Unusable,
        },
    }
}

/// Разбор байтов журнала; не тот формат — `None`. Всё после последнего
/// `\n` — оборванная запись, отбрасывается.
fn parse(bytes: &[u8]) -> Option<Journal> {
    let complete = &bytes[..bytes.iter().rposition(|&b| b == b'\n')?];
    let mut lines = complete.split(|&b| b == b'\n');
    let header: HeaderIn = serde_json::from_slice(lines.next()?).ok()?;
    if header.format_version != FORMAT_VERSION {
        return None;
    }
    let total = header.notes.len();
    let mut done = HashSet::new();
    let mut started: HashMap<usize, Vec<Started>> = HashMap::new();
    for line in lines {
        match serde_json::from_slice::<Mark<String>>(line).ok()? {
            Mark::Done(note) if note < total => {
                done.insert(note);
            }
            Mark::Start(mark) if mark.note < total => {
                started.entry(mark.note).or_default().push(Started {
                    before: mark.before,
                    after: mark.after,
                });
            }
            _ => return None,
        }
    }
    Some(Journal {
        renames: header.renames,
        notes: header
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
        done,
        started,
    })
}

/// Удаляет журнал (план выполнен или журнал не годится).
pub fn remove(vault: &Vault) -> CoreResult<()> {
    vault.remove_service_file(FILE)
}

#[cfg(test)]
thread_local! {
    static BYTES_WRITTEN: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Учёт записанного в журнал в этом потоке (замер; вне тестов — ничего).
fn count(bytes: usize) {
    #[cfg(test)]
    BYTES_WRITTEN.with(|cell| cell.set(cell.get() + bytes as u64));
    let _ = bytes;
}

/// Сбрасывает счётчик записанного в журнал (замер).
#[cfg(test)]
pub fn reset_bytes_written() {
    BYTES_WRITTEN.with(|cell| cell.set(0));
}

/// Сколько байт записано в журнал в этом потоке с последнего сброса.
#[cfg(test)]
pub fn bytes_written() -> u64 {
    BYTES_WRITTEN.with(std::cell::Cell::get)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(path: &str) -> VaultPath {
        VaultPath::from_components(path.split('/'))
    }

    #[test]
    fn round_trip_keeps_renames_marks_and_triples() {
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

        let mut writer = start(&vault, &renames, &notes).expect("заголовок");
        writer.started(0, "aaa", "bbb").expect("отметка");
        writer.done(0);
        drop(writer);

        let Read::Found(journal) = read(&vault) else {
            panic!("журнал не прочитан");
        };
        assert_eq!(journal.renames, renames);
        assert_eq!(journal.notes, notes);
        assert_eq!(journal.done, HashSet::from([0]));
        assert_eq!(
            journal.started[&0],
            [Started {
                before: "aaa".into(),
                after: "bbb".into()
            }]
        );
        let text =
            std::fs::read_to_string(dir.path().join(".rhizomind").join(FILE.name())).expect("файл");
        assert!(text.starts_with(r#"{"format_version":1,"#), "{text}");
        assert!(text.ends_with("{\"done\":0}\n"), "{text}");
        remove(&vault).expect("удаление");
        assert!(matches!(read(&vault), Read::Missing));
    }

    #[test]
    fn torn_last_line_is_dropped_and_middle_garbage_rejected() {
        let header = br#"{"format_version":1,"renames":[],"notes":[{"path":["X.md"],"links":[]}]}"#;
        let mut ok = header.to_vec();
        ok.extend_from_slice(b"\n{\"done\":0}\n{\"start\":{\"no");
        let journal = parse(&ok).expect("оборванная строка отброшена");
        assert_eq!(journal.done, HashSet::from([0]));
        assert!(journal.started.is_empty());

        let mut bad = header.to_vec();
        bad.extend_from_slice(b"\n{\"don\n{\"done\":0}\n");
        assert!(parse(&bad).is_none(), "испорченная строка в середине");
        assert!(
            parse(header).is_none(),
            "заголовок без конца строки — оборван"
        );
    }
}
