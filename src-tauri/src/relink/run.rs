//! Выполнение плана переписывания ссылок (задача 0.2-4.5; С2-3.5 «Где
//! ссылки не могут быть обновлены», С2-5.4 шаги 3–5, «Отката нет», Р6, Р8,
//! решение 115).
//!
//! Порядок ([`run`]):
//! 1. заголовок журнала `.rhizomind/link-update.json` с планом
//!    ([`super::journal`]); не записался — ни одна заметка не
//!    переписывается, все в `failed` с `io` (их можно повторить);
//! 2. каждая заметка — [`Vault::write_note_rebuilt`]: свежий текст с диска,
//!    ссылки находятся заново ([`rewrite`]), запись атомарно с проверкой
//!    версии, до 3 попыток, своя запись — в журнал своих записей;
//! 3. перед заменой заметки в журнал дописывается отметка «начата» с хэшами
//!    прочитанного и нового содержимого (со сбросом на диск), после —
//!    «готова»; объём журнала на заметку — O(1);
//! 4. в конце журнал удаляется.
//!
//! Какие ссылки переписываются (ревью 0.2-4.3, «Для 4.5 и 4.6»): цель в
//! свежем тексте равна `old_target`, по списку заметок «после» она **не**
//! ведёт однозначно в `points_to`, а `new_target` — ведёт. Уже переписанная,
//! поправленная пользователем или ведущая куда надо ссылка не трогается;
//! правка, которая не привела бы ссылку на нужную заметку, не делается
//! (защита и от подменённого журнала).
//!
//! Отката нет: уже переписанные заметки остаются переписанными, каждая верна
//! сама по себе. Пропуски — `too_large`, `not_utf8`, `busy`, `io`
//! ([`RelinkFailureReason`]); в [`RunReport::retry`] — только `busy` и `io`
//! (решение 115), повтор — тот же [`run`] по этому плану на свежем тексте.
//!
//! После падения посреди плана — [`resume_on_open`] при следующем открытии
//! хранилища: те же шаги по заметкам журнала без отметки «готова». Для
//! заметки с отметкой «начата» сначала сверяется хэш на диске: записанное
//! содержимое — заметка готова; прочитанное — переписывается; ни то ни другое
//! (заметку поправили снаружи после записи) — в `failed` с `conflict` (Р20)
//! **без повтора**: по тексту не отличить уже переписанную ссылку от
//! непереписанной (после перестановки папок новая цель одной ссылки
//! совпадает со старой целью другой), а переписывание вслепую увело бы
//! ссылку не туда.
//!
//! Одновременно выполняется один план на хранилище: журнал один, вызывающий
//! (команды 0.2-4.6, 0.2-6.1) выполняет планы по очереди.

use std::collections::HashMap;

use super::journal::{self, Read, Started, Writer};
use super::{NotePlan, PlannedLink, RelinkPlan, rewrite};
use crate::commands::{
    ErrorCode, FileVersion, PathRename, RelinkFailure, RelinkFailureReason, RelinkOutcome,
    RelinkToken, Settings, VaultPath,
};
use crate::links::{Links, NotesView};
use crate::markdown::WikiLink;
use crate::vault::{Rebuilt, Vault};

/// Итог выполнения плана.
#[derive(Debug, Default)]
pub struct RunReport {
    /// Переписанные заметки и их новые версии — для `note:changed` и
    /// перечитывания индекса.
    pub updated: Vec<(VaultPath, FileVersion)>,
    /// Заметки, которые переписать не удалось, с причиной.
    pub failed: Vec<RelinkFailure>,
    /// План повтора (решение 115): только заметки с `busy` и `io`, с их
    /// ссылками; пусто — повторять нечего, `retry_token` не нужен.
    pub retry: RelinkPlan,
    /// Несопоставленные переименования плана ([`RelinkPlan::unmatched`]):
    /// по ним ничего не переписано — это сбой, не тихий успех.
    pub unmatched: Vec<PathRename>,
}

impl RunReport {
    /// Ответ для `relink` и `links:updated`. Несопоставленное
    /// переименование — в `failed` с причиной `conflict` (Р20) и путём
    /// «куда» (ссылки на него не обновлены, повтор не поможет). `retry_token` вызывающий выдаёт, только если
    /// [`Self::retry`] не пуст.
    pub fn outcome(&self, retry_token: Option<RelinkToken>) -> RelinkOutcome {
        let mut failed = self.failed.clone();
        failed.extend(self.unmatched.iter().map(|rename| RelinkFailure {
            path: rename.to.clone(),
            reason: RelinkFailureReason::Conflict,
        }));
        RelinkOutcome {
            updated: u32::try_from(self.updated.len()).unwrap_or(u32::MAX),
            failed,
            retry_token,
        }
    }
}

/// Выполняет план `plan` (или план повтора [`RunReport::retry`]): журнал,
/// запись заметок, удаление журнала. `after` — список заметок после
/// переименований `renames` (их вызывающий уже выполнил на диске);
/// `renames` пишутся в журнал. `progress(сделано, всего)` — перед каждой
/// заметкой и в конце (для `links:progress`, частоту ограничивает
/// вызывающий). Блокирует поток (диск).
pub fn run(
    vault: &Vault,
    after: &NotesView<'_>,
    plan: &RelinkPlan,
    renames: &[PathRename],
    progress: &mut dyn FnMut(usize, usize),
) -> RunReport {
    let mut report = RunReport {
        unmatched: plan.unmatched.clone(),
        ..RunReport::default()
    };
    if plan.notes.is_empty() {
        return report;
    }
    let Ok(mut writer) = journal::start(vault, renames, &plan.notes) else {
        // Без журнала доделка после падения невозможна: не начинать.
        for note in &plan.notes {
            report.fail(note, RelinkFailureReason::Io);
        }
        return report;
    };
    let todo: Vec<usize> = (0..plan.notes.len()).collect();
    let finished = execute(
        vault,
        after,
        &plan.notes,
        &todo,
        &HashMap::new(),
        &mut writer,
        progress,
        &mut report,
    );
    finish(vault, writer, finished);
    report
}

/// Доделка обновления ссылок, прерванного при прошлом запуске (С2-5.4,
/// «Отката нет»). Вызывать при открытии хранилища **после сверки индекса**,
/// до работы окна с заметками; `notes` — список заметок хранилища сейчас.
/// Журнала нет — `None`. Журнал не годится (испорчен, другая версия, больше
/// предела) — убирается, `None`. Иначе — оставшиеся заметки журнала
/// переписываются по тем же правилам, что в [`run`], журнал удаляется;
/// итог — для `links:updated` с `reason: resumed` («Обновление ссылок,
/// прерванное при прошлом запуске, завершено»).
///
/// Журнал — недоверенные данные: заметки с путём, который не проходит
/// правила контура, пропускаются молча; запись идёт только через контур
/// хранилища; ссылка переписывается, только если новая цель однозначно
/// ведёт на заметку из журнала.
pub fn resume_on_open(
    vault: &Vault,
    notes: &Links,
    progress: &mut dyn FnMut(usize, usize),
) -> Option<RunReport> {
    let journal = match journal::read(vault) {
        Read::Missing | Read::Refused => return None,
        Read::Unusable => {
            let _ = journal::remove(vault);
            return None;
        }
        Read::Found(journal) => journal,
    };
    let Ok(mut writer) = journal::reopen(vault) else {
        return None;
    };
    let todo: Vec<usize> = (0..journal.notes.len())
        .filter(|i| !journal.done.contains(i) && trusted_note_path(&journal.notes[*i].path))
        .collect();
    let mut report = RunReport::default();
    let after = notes.view();
    let finished = execute(
        vault,
        &after,
        &journal.notes,
        &todo,
        &journal.started,
        &mut writer,
        progress,
        &mut report,
    );
    finish(vault, writer, finished);
    Some(report)
}

/// Закрывает журнал и удаляет его, если план пройден до конца.
fn finish(vault: &Vault, writer: Writer, finished: bool) {
    drop(writer);
    if finished {
        let _ = journal::remove(vault);
    }
}

impl RunReport {
    fn fail(&mut self, note: &NotePlan, reason: RelinkFailureReason) {
        self.failed.push(RelinkFailure {
            path: note.path.clone(),
            reason,
        });
        if reason.is_retryable() {
            self.retry.notes.push(note.clone());
        }
    }

    /// Сбой без повтора (Р20, `conflict`): заметку поправили снаружи после
    /// начатой записи.
    fn fail_unverified(&mut self, note: &NotePlan) {
        self.failed.push(RelinkFailure {
            path: note.path.clone(),
            reason: RelinkFailureReason::Conflict,
        });
    }
}

/// Путь из журнала годится: проходит правила контура, это имя заметки, и
/// он не ведёт в служебную папку. Остальное отбрасывается молча.
fn trusted_note_path(path: &VaultPath) -> bool {
    path.check().is_ok()
        && path.file_name().is_some_and(crate::names::is_note)
        && !path
            .components()
            .iter()
            .any(|c| crate::vault::is_service_dir(c))
}

/// Как поступить с заметкой, запись которой была начата до падения.
enum Resumed {
    /// На диске записанное содержимое — заметка готова.
    Written(FileVersion),
    /// На диске прочитанное содержимое или файла нет — выполнять как обычно.
    Proceed,
    /// Ни то ни другое — не переписывать вслепую.
    Unverified,
}

/// Сверка хэша на диске с начатыми записями заметки.
fn check_started(vault: &Vault, path: &VaultPath, started: &[Started]) -> Resumed {
    match vault.file_version(path) {
        Ok(version) if started.iter().any(|s| s.after == version.hash) => Resumed::Written(version),
        Ok(version) if started.iter().any(|s| s.before == version.hash) => Resumed::Proceed,
        Err(error) if error.code == ErrorCode::NotFound => Resumed::Proceed,
        _ => Resumed::Unverified,
    }
}

/// Шаги 2–3 по заметкам `notes` с номерами `todo`; заголовок журнала уже на
/// диске. `started` — начатые до падения записи (доделка). `false` —
/// выполнение прервано (тестовый шов падения).
#[allow(clippy::too_many_arguments)]
fn execute(
    vault: &Vault,
    after: &NotesView<'_>,
    notes: &[NotePlan],
    todo: &[usize],
    started: &HashMap<usize, Vec<Started>>,
    writer: &mut Writer,
    progress: &mut dyn FnMut(usize, usize),
    report: &mut RunReport,
) -> bool {
    let limit = vault
        .read_settings()
        .map(|settings| settings.max_edit_size_bytes)
        .unwrap_or_else(|_| Settings::default().max_edit_size_bytes);
    let total = todo.len();
    for (done, &i) in todo.iter().enumerate() {
        progress(done, total);
        let note = &notes[i];
        if let Some(started) = started.get(&i) {
            match check_started(vault, &note.path, started) {
                Resumed::Written(version) => {
                    report.updated.push((note.path.clone(), version));
                    writer.done(i);
                    continue;
                }
                Resumed::Unverified => {
                    report.fail_unverified(note);
                    continue;
                }
                Resumed::Proceed => {}
            }
        }
        let build = |text: &str| {
            rewrite(text, &|link: &WikiLink| {
                new_target(after, &note.links, &link.target)
            })
        };
        let mut mark = |before: &str, after: &str| writer.started(i, before, after);
        match vault.write_note_rebuilt(&note.path, limit, &build, &mut mark) {
            Ok(Rebuilt::Written(version)) => {
                report.updated.push((note.path.clone(), version));
                if crashed(report.updated.len()) {
                    return false;
                }
                writer.done(i);
            }
            Ok(Rebuilt::Unchanged | Rebuilt::Gone) => writer.done(i),
            Err(reason) => report.fail(note, reason),
        }
    }
    progress(total, total);
    true
}

/// На что переписать ссылку с целью `target` (как в свежем тексте): тройка
/// с `old_target == target`, ссылка сейчас не ведёт однозначно в
/// `points_to`, а `new_target` — ведёт. Иначе `None` — не трогать.
fn new_target(after: &NotesView<'_>, links: &[PlannedLink], target: &str) -> Option<String> {
    let link = links.iter().find(|link| link.old_target == target)?;
    let there = std::slice::from_ref(&link.points_to);
    if after.candidates(target).as_slice() == there {
        return None;
    }
    (after.candidates(&link.new_target).as_slice() == there).then(|| link.new_target.clone())
}

#[cfg(test)]
thread_local! {
    static CRASH_AFTER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Тестовый шов: выполнение «падает» (выходит, не удаляя журнал) после
/// замены `n`-й записанной заметки в этом потоке, до отметки «готова».
#[cfg(test)]
pub(crate) fn crash_after(n: usize) {
    CRASH_AFTER.with(|cell| cell.set(Some(n)));
}

/// Точка шва [`crash_after`]; вне тестов — `false`.
fn crashed(written: usize) -> bool {
    #[cfg(test)]
    if CRASH_AFTER.with(|cell| cell.get()) == Some(written) {
        CRASH_AFTER.with(|cell| cell.set(None));
        return true;
    }
    let _ = written;
    false
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
