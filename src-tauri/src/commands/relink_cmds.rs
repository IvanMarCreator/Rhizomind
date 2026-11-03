//! Команды обновления ссылок (0.2 «Связи», С2-3.5, С2-3.6, С2-5.4, С2-5.5).
//!
//! Интерфейс не может попросить ядро переписать произвольный файл: план
//! переписывания всегда считает ядро, интерфейс получает только число
//! заметок и ссылок и непрозрачный знак плана ([`RelinkToken`]) и может лишь
//! подтвердить или отклонить план (С2-6 «Граница»). Списка файлов в запросах
//! нет, лишнее поле отклоняется (`deny_unknown_fields`).
//!
//! Порядок для переименования в приложении (С2-5.4): `relink_preview` →
//! ответ пользователя → `entry_rename`/`entry_move` с `links: update` и
//! знаком → событие `links:flush-requested` → `links_flush_done` → файл
//! переименован → ссылки переписаны → ответ с `relink` и `links:updated`.
//!
//! Порядок (С2-5.4 «Порядок для переименования в приложении», задача
//! 0.2-4.6): знак предпросмотра проверяется до любого действия; планы одного
//! хранилища выполняются строго по очереди, общей для всех окон одного
//! хранилища (журнал `link-update.json` один, [`Relinker::serial`]); ввод
//! открытых заметок записывается **до** расчёта плана и переименования
//! (инвариант 4) — без ответа окна за [`FLUSH_TIMEOUT`] или с ответом «не
//! записано» переименование не выполняется (`io_failed`, знак остаётся
//! действующим); не удалось переименовать — ни один файл не переписан. План считается заново по индексу и списку заметок
//! после записи ввода: состав заметок или ссылки могли измениться после
//! предпросмотра (тот же план, если ничего не менялось).
//!
//! Итог — одно событие `links:updated` (уведомление показывается только по
//! нему), `note:changed` для каждой переписанной заметки (своя запись ядра
//! не приходит от наблюдателя), ход — `links:progress` не чаще
//! [`limits::MAX_EVENTS_PER_SEC`] раз в секунду.
//!
//! Внешние переименования (Проводник, задача 0.2-6.1) — [`ExternalRenames`]:
//! свой поток, та же очередь планов, вопрос `links:pending` или сразу
//! обновление; ответ — [`relink_answer`].
//!
//! Доделка после сбоя ([`resume_after_reconcile`]) — при открытии хранилища
//! после сверки индекса и до того, как индекс станет годен для
//! переименований с обновлением ссылок (признак `reconciled`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, OnceLock, PoisonError, Weak, mpsc};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{Manager, Runtime, State, Window};

use super::error::{CoreError, CoreResult, ErrorCode};
use super::events::{
    LinksFlushRequestedPayload, LinksPendingPayload, LinksProgressPayload, LinksUpdateReason,
    LinksUpdatedPayload, NoteChangedPayload, PathRename, PendingReason, RelinkFailure,
    RelinkFailureReason, RelinkOutcome, TreeChange, event,
};
use super::files::EntryMovedResponse;
use super::index_cmds::{IndexProgress, IndexReply, Throttle};
use super::limits;
use super::settings::RenameLinks;
use super::types::{RelinkToken, VaultPath};
use super::{Core, OpenVault, blocking, check_entry_name, checked_path};
use crate::events::{Emit, WindowEmitter, send};
use crate::links::Links;
use crate::relink::tokens::{self, Tokens};
use crate::relink::{self, RelinkPlan, RunReport};

/// Запрос `relink_preview`: какое переименование или перемещение
/// пользователь собирается сделать. В JSON — поле `op`:
/// `{ "op": "rename", "path": [...], "new_name": "..." }` или
/// `{ "op": "move", "path": [...], "new_dir": [...] }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum RelinkPreviewRequest {
    /// Переименование заметки или папки (параметры как у `entry_rename`).
    Rename {
        /// Что переименовать.
        path: VaultPath,
        /// Новое полное имя, как на диске (для заметки — с `.md`).
        new_name: String,
    },
    /// Перемещение заметки или папки (параметры как у `entry_move`).
    Move {
        /// Что переместить.
        path: VaultPath,
        /// Папка назначения (`[]` — корень).
        new_dir: VaultPath,
    },
}

/// Предпросмотр обновления ссылок (`relink_preview`, вариант `ready`).
/// `notes: 0` — переписывать нечего: интерфейс переименовывает сразу, без
/// окна и с `links: keep` (С2-3.5). Уточнение ссылок при повторе имени,
/// вызванное этим переименованием, в числа не входит и от ответа не зависит
/// (план 0.2, Т6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelinkPreview {
    /// Сколько заметок ссылается на переименуемое (будут переписаны).
    pub notes: u32,
    /// Сколько ссылок будет переписано.
    pub links: u32,
    /// Знак плана для `entry_rename`/`entry_move` с `links: update`.
    pub token: RelinkToken,
}

/// Запрос `relink_answer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelinkAnswerRequest {
    /// Знак из `links:pending` (вопрос о внешних переименованиях или о
    /// повторах имён) или `retry_token` из `links:updated`/`relink`
    /// («Повторить», решение 115).
    pub token: RelinkToken,
    /// `true` — «Обновить ссылки», «Уточнить», «Повторить»; `false` — «Не
    /// обновлять», «Не трогать» (план отбрасывается).
    pub apply: bool,
}

/// Запрос `links_flush_done`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinksFlushDoneRequest {
    /// Знак из `links:flush-requested`.
    pub token: RelinkToken,
    /// `true` — несохранённый ввод всех заметок окна записан; `false` —
    /// записать не удалось: ссылки не переписываются, как и без ответа за
    /// 10 с (инвариант 4).
    pub saved: bool,
}

/// Проверка операции предпросмотра — те же правила, что у `entry_rename` и
/// `entry_move`.
fn check_preview(request: &RelinkPreviewRequest) -> CoreResult<()> {
    match request {
        RelinkPreviewRequest::Rename { path, new_name } => {
            checked_path(path, "path")?;
            check_entry_name(new_name, "new_name")?;
            let old_name = path
                .file_name()
                .ok_or_else(|| CoreError::invalid_argument("path"))?;
            match crate::names::check_rename_keeps_kind(old_name, new_name) {
                Some(problem) => Err(CoreError::invalid_name(problem)),
                None => Ok(()),
            }
        }
        RelinkPreviewRequest::Move { path, new_dir } => {
            checked_path(path, "path")?;
            if path.components().is_empty() {
                return Err(CoreError::invalid_argument("path"));
            }
            checked_path(new_dir, "new_dir")
        }
    }
}

/// Сколько окно записывает несохранённый ввод перед переписыванием
/// (С2-5.4, шаг 2; как при закрытии окна).
pub const FLUSH_TIMEOUT: Duration = Duration::from_secs(10);

/// Что стоит за знаком плана, выданным окну.
#[derive(Debug)]
pub enum Grant {
    /// Предпросмотр: переименование или перемещение, на которое пользователь
    /// отвечает. План по нему ядро считает само, заново при выполнении.
    Preview(PathRename),
    /// «Повторить» (решение 115): план повтора — только заметки с `busy` и
    /// `io` — и переименования исходного плана (для журнала и
    /// `links:updated`).
    Retry {
        /// План повтора.
        plan: RelinkPlan,
        /// Переименования исходного плана.
        renames: Vec<PathRename>,
    },
    /// Вопрос `links:pending` о внешних переименованиях (решение 110): план
    /// по ним ядро считает заново при ответе — состав заметок и ссылки
    /// могли измениться (задача 0.2-6.1).
    External(Vec<PathRename>),
}

/// Очереди планов по хранилищам: путь корня → общий замок. Окно закрыли и
/// сразу открыли снова — новое окно получает тот же замок, пока старое
/// выполнение его держит (запись живёт, пока жив хоть один `Arc`).
static QUEUES: LazyLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> = LazyLock::new(Mutex::default);

/// Общая очередь планов хранилища с корнем `root` (путь уже канонический —
/// `Vault::root`).
fn queue_for(root: &Path) -> Arc<Mutex<()>> {
    let mut queues = QUEUES.lock().unwrap_or_else(PoisonError::into_inner);
    queues.retain(|_, queue| queue.strong_count() > 0);
    if let Some(queue) = queues.get(root).and_then(Weak::upgrade) {
        return queue;
    }
    let queue = Arc::new(Mutex::new(()));
    queues.insert(root.to_path_buf(), Arc::downgrade(&queue));
    queue
}

/// Знак предпросмотра, взятый для выполнения: его можно вернуть, если
/// выполнение не началось ([`Relinker::restore_preview`]).
#[derive(Debug)]
pub struct PreviewTicket {
    token: RelinkToken,
    issued: Instant,
}

impl PreviewTicket {
    /// Знак (для `links:progress`).
    pub fn token(&self) -> &RelinkToken {
        &self.token
    }
}

/// Обновление ссылок в работе окна хранилища: знаки планов, ожидание
/// записи ввода, очередь планов.
#[derive(Debug)]
pub struct Relinker {
    /// Окно хранилища — знаки привязаны к нему.
    window: OnceLock<String>,
    /// Знаки предпросмотра и повтора.
    grants: Tokens<Grant>,
    /// Ожидания записи ввода: знак → ответ окна (`None` — ещё нет).
    flushes: Mutex<HashMap<String, Option<bool>>>,
    flushed: Condvar,
    /// Планы и переименования — строго по очереди, общей для всех окон
    /// одного хранилища ([`queue_for`]).
    serial: Arc<Mutex<()>>,
    /// Предел ожидания записи ввода ([`FLUSH_TIMEOUT`]; в тестах короче).
    flush_timeout: Mutex<Duration>,
}

impl Default for Relinker {
    fn default() -> Self {
        Self {
            window: OnceLock::new(),
            grants: Tokens::default(),
            flushes: Mutex::default(),
            flushed: Condvar::new(),
            serial: Arc::new(Mutex::new(())),
            flush_timeout: Mutex::new(FLUSH_TIMEOUT),
        }
    }
}

impl Relinker {
    /// Обновление ссылок хранилища с корнем `root`: очередь планов — общая
    /// для всех окон этого хранилища.
    pub fn for_root(root: &Path) -> Self {
        Self {
            serial: queue_for(root),
            ..Self::default()
        }
    }

    /// Привязка к окну хранилища (один раз, при открытии).
    pub fn bind_window(&self, label: &str) {
        let _ = self.window.set(label.to_owned());
    }

    /// Окно хранилища; не привязано — пустая строка (такой метки у окон нет,
    /// знак не примет никто).
    fn window(&self) -> &str {
        self.window.get().map_or("", String::as_str)
    }

    /// Очередь планов хранилища: держится от записи ввода до
    /// `links:updated`; переименования без обновления ссылок тоже ждут её.
    pub fn serial(&self) -> MutexGuard<'_, ()> {
        self.serial.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn flushes(&self) -> MutexGuard<'_, HashMap<String, Option<bool>>> {
        self.flushes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Знак окну хранилища.
    fn issue(&self, grant: Grant) -> CoreResult<RelinkToken> {
        self.grants.issue(self.window(), grant)
    }

    /// Знак, предъявленный окном `window`: то, что за ним стоит. Неизвестный,
    /// использованный, чужой или просроченный — `invalid_argument` с полем
    /// `field`. Знак сгорает при любом предъявлении.
    fn take(&self, window: &str, token: &RelinkToken, field: &str) -> CoreResult<Grant> {
        limits::token(token, field)?;
        if window != self.window() {
            // Знак сгорает и здесь.
            let _ = self.grants.take(window, token);
            return Err(CoreError::invalid_argument(field));
        }
        self.grants
            .take(window, token)
            .ok_or_else(|| CoreError::invalid_argument(field))
    }

    /// Знак предпросмотра для переименования `rename`: выдан этому окну, не
    /// просрочен, не использован и выдан **для этого же** переименования
    /// (иначе — `invalid_argument`, `token`).
    pub fn take_preview(
        &self,
        window: &str,
        token: &RelinkToken,
        rename: &PathRename,
    ) -> CoreResult<PreviewTicket> {
        let refused = || CoreError::invalid_argument("token");
        limits::token(token, "token")?;
        if window != self.window() {
            let _ = self.grants.take(window, token);
            return Err(refused());
        }
        match self.grants.take_issued(window, token).ok_or_else(refused)? {
            (Grant::Preview(previewed), issued) if previewed == *rename => Ok(PreviewTicket {
                token: token.clone(),
                issued,
            }),
            _ => Err(refused()),
        }
    }

    /// Возвращает знак предпросмотра, по которому ничего не сделано (ввод не
    /// записан): тот же знак, то же переименование, прежний срок — окно
    /// может попробовать снова.
    fn restore_preview(&self, ticket: PreviewTicket, rename: &PathRename) {
        self.grants.restore(
            self.window(),
            &ticket.token,
            Grant::Preview(rename.clone()),
            ticket.issued,
        );
    }

    /// Окно записывает несохранённый ввод всех своих заметок:
    /// `links:flush-requested` и ожидание `links_flush_done` не дольше
    /// предела. `true` — записано; `false` — не записано или нет ответа.
    /// Блокирует поток.
    pub fn flush(&self, emit: &dyn Emit) -> bool {
        let Ok(token) = tokens::fresh() else {
            return false;
        };
        let timeout = *self
            .flush_timeout
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.flushes().insert(token.0.clone(), None);
        send(
            emit,
            event::LINKS_FLUSH_REQUESTED,
            &LinksFlushRequestedPayload {
                token: token.clone(),
            },
        );
        let deadline = Instant::now() + timeout;
        let mut flushes = self.flushes();
        loop {
            if let Some(Some(saved)) = flushes.get(&token.0) {
                let saved = *saved;
                flushes.remove(&token.0);
                return saved;
            }
            let now = Instant::now();
            if now >= deadline {
                // Поздний ответ получит `invalid_argument`.
                flushes.remove(&token.0);
                return false;
            }
            flushes = self
                .flushed
                .wait_timeout(flushes, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Ответ окна на `links:flush-requested`. Знака нет среди ожиданий
    /// (неизвестный, поздний, уже отвеченный) — `false`.
    pub fn flush_done(&self, token: &RelinkToken, saved: bool) -> bool {
        let mut flushes = self.flushes();
        match flushes.get_mut(&token.0) {
            Some(answer @ None) => {
                *answer = Some(saved);
                drop(flushes);
                self.flushed.notify_all();
                true
            }
            _ => false,
        }
    }

    /// Занять очередь планов (тесты).
    #[cfg(test)]
    pub fn lock_queue(&self) -> MutexGuard<'_, ()> {
        self.serial()
    }

    /// Очередь занята (тесты).
    #[cfg(test)]
    pub fn queue_busy(&self) -> bool {
        self.serial.try_lock().is_err()
    }

    /// Предел ожидания записи ввода (тесты).
    #[cfg(test)]
    pub fn set_flush_timeout(&self, timeout: Duration) {
        *self
            .flush_timeout
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = timeout;
    }

    /// Часы знаков уходят вперёд (тесты просрочки).
    #[cfg(test)]
    pub fn advance_tokens(&self, by: Duration) {
        self.grants.advance(by);
    }
}

/// Путь после переименования `path` в `new_name` (та же папка).
pub fn renamed_path(path: &VaultPath, new_name: &str) -> VaultPath {
    let parent = &path.components()[..path.components().len().saturating_sub(1)];
    VaultPath::from_components(parent.iter().cloned().chain([new_name.to_owned()]))
}

/// Путь после перемещения `path` в папку `new_dir`.
pub fn moved_path(path: &VaultPath, new_dir: &VaultPath) -> VaultPath {
    VaultPath::from_components(
        new_dir
            .components()
            .iter()
            .cloned()
            .chain(path.file_name().map(str::to_owned)),
    )
}

impl RelinkPreviewRequest {
    /// Переименование «откуда → куда» этой операции.
    pub fn rename(&self) -> PathRename {
        match self {
            Self::Rename { path, new_name } => PathRename {
                from: path.clone(),
                to: renamed_path(path, new_name),
            },
            Self::Move { path, new_dir } => PathRename {
                from: path.clone(),
                to: moved_path(path, new_dir),
            },
        }
    }
}

/// План для переименования `rename` по индексу и списку заметок сейчас —
/// под замками «список заметок → индекс», без чтения диска; план отдаётся
/// без ссылок на общее состояние. Не заметка и папка без заметок — пустой
/// план (переписывать нечего, это не сбой).
fn plan_for(open: &OpenVault, rename: &PathRename) -> RelinkPlan {
    let list = open.read_notes();
    if list.notes_under(&rename.from).is_empty() {
        return RelinkPlan::default();
    }
    open.index
        .with_index(|index| relink::plan(index, list.links(), std::slice::from_ref(rename), &[]))
}

/// `links:progress` не чаще [`limits::MAX_EVENTS_PER_SEC`] раз в секунду.
pub struct ProgressEvents<'a> {
    emit: &'a dyn Emit,
    token: Option<RelinkToken>,
    gate: Throttle,
}

impl<'a> ProgressEvents<'a> {
    /// Ход плана со знаком `token` (если план выдавался окну) — в `emit`.
    pub fn new(emit: &'a dyn Emit, token: Option<RelinkToken>) -> Self {
        Self {
            emit,
            token,
            gate: Throttle::new(Duration::from_millis(
                1_000 / u64::from(limits::MAX_EVENTS_PER_SEC),
            )),
        }
    }

    /// Обработано `done` из `total` к моменту `now`. Чаще предела — молча
    /// пропускается: конец хода обозначает `links:updated`.
    pub fn on(&mut self, done: usize, total: usize, now: Instant) {
        if total == 0 || !self.gate.try_fire(now) {
            return;
        }
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        send(
            self.emit,
            event::LINKS_PROGRESS,
            &LinksProgressPayload {
                token: self.token.clone(),
                done: count(done),
                total: count(total),
            },
        );
    }
}

/// Список заметок сейчас — собственная копия для выполнения плана: замок
/// списка заметок не держится, пока пишутся файлы.
fn current_links(open: &OpenVault) -> Links {
    Links::new(open.read_notes().note_refs())
}

/// Итог выполнения: индекс перечитывает переписанные заметки, окну —
/// `note:changed` по каждой и одно `links:updated`; знак повтора — если
/// есть заметки с `busy`/`io`.
fn publish(
    open: &OpenVault,
    emit: &dyn Emit,
    report: &RunReport,
    renames: &[PathRename],
    reason: LinksUpdateReason,
) -> RelinkOutcome {
    let changes: Vec<TreeChange> = report
        .updated
        .iter()
        .map(|(path, _)| TreeChange::Modified { path: path.clone() })
        .collect();
    open.apply_own(&changes);
    for (path, version) in &report.updated {
        send(
            emit,
            event::NOTE_CHANGED,
            &NoteChangedPayload {
                path: path.clone(),
                version: version.clone(),
            },
        );
    }
    let retry_token = if report.retry.notes.is_empty() {
        None
    } else {
        open.relink
            .issue(Grant::Retry {
                plan: report.retry.clone(),
                renames: renames.to_vec(),
            })
            .ok()
    };
    let outcome = report.outcome(retry_token);
    send(
        emit,
        event::LINKS_UPDATED,
        &LinksUpdatedPayload::new(reason, outcome.clone(), renames),
    );
    outcome
}

/// Выполняет план `plan` (очередь планов уже занята, ввод записан):
/// переписывание и [`publish`]. Пустой план — ничего не пишется, событий
/// нет.
fn execute(
    open: &OpenVault,
    emit: &dyn Emit,
    plan: &RelinkPlan,
    renames: &[PathRename],
    reason: LinksUpdateReason,
    token: Option<RelinkToken>,
) -> RelinkOutcome {
    if plan.is_empty() {
        return RelinkOutcome::default();
    }
    let after = current_links(open);
    let mut progress = ProgressEvents::new(emit, token);
    let report = relink::run(
        &open.vault,
        &after.view(),
        plan,
        renames,
        &mut |done, total| progress.on(done, total, Instant::now()),
    );
    publish(open, emit, &report, renames, reason)
}

/// Ошибка «ввод окна не записан» (`io_failed`): переименование не
/// выполнено, на диске ничего не изменилось.
fn flush_failed() -> CoreError {
    CoreError::new(ErrorCode::IoFailed)
}

/// Переименование или перемещение с обновлением ссылок (`entry_rename`,
/// `entry_move` с `links: update`; знак уже взят — `ticket`): очередь планов
/// → запись ввода → план → `act` (переименование на диске) → переписывание.
///
/// Ввод не записан (окно ответило «не записано» или не ответило за
/// предел) — переименование **не выполняется** (С2-5.4: запись ввода
/// раньше переименования; решение 114: ссылки ни на миг не ведут не туда),
/// ответ — `io_failed`, на диске ничего не изменилось, знак предпросмотра
/// возвращается действующим с прежним сроком. `act` не удался — его
/// ошибка, ни один файл не переписан. Блокирует поток.
pub fn rename_with_links(
    open: &OpenVault,
    emit: &dyn Emit,
    rename: &PathRename,
    ticket: PreviewTicket,
    act: impl FnOnce() -> CoreResult<VaultPath>,
) -> CoreResult<EntryMovedResponse> {
    let _serial = open.relink.serial();
    if !open.relink.flush(emit) {
        open.relink.restore_preview(ticket, rename);
        return Err(flush_failed());
    }
    // План — после записи ввода: записанный ввод мог добавить ссылки.
    let plan = plan_for(open, rename);
    let path = act()?;
    open.apply_own(&[TreeChange::Renamed {
        from: rename.from.clone(),
        to: path.clone(),
    }]);
    let renames = [PathRename {
        from: rename.from.clone(),
        to: path.clone(),
    }];
    let relink = execute(
        open,
        emit,
        &plan,
        &renames,
        LinksUpdateReason::Rename,
        Some(ticket.token().clone()),
    );
    Ok(EntryMovedResponse {
        path,
        relink: Some(relink),
    })
}

/// Ввод окна не записан, а переименование уже на диске («Повторить» или
/// внешнее переименование): ни одна заметка не переписана, но это не
/// теряется молча — `links:updated` (`reason`) с заметками плана как `busy`
/// (их можно повторить) и знаком повтора на этот план.
fn not_started(
    open: &OpenVault,
    emit: &dyn Emit,
    plan: RelinkPlan,
    renames: &[PathRename],
    reason: LinksUpdateReason,
) {
    let failed: Vec<RelinkFailure> = plan
        .notes
        .iter()
        .map(|note| RelinkFailure {
            path: note.path.clone(),
            reason: RelinkFailureReason::Busy,
        })
        .collect();
    let retry_token = open
        .relink
        .issue(Grant::Retry {
            plan,
            renames: renames.to_vec(),
        })
        .ok();
    let outcome = RelinkOutcome {
        updated: 0,
        failed,
        retry_token,
    };
    send(
        emit,
        event::LINKS_UPDATED,
        &LinksUpdatedPayload::new(reason, outcome, renames),
    );
}

/// Сколько поток внешних переименований ждёт между проверками готовности
/// индекса.
const READY_POLL: Duration = Duration::from_millis(50);

/// Внешние переименования хранилища (задача 0.2-6.1; С2-3.5 «в
/// Проводнике», Р10, решение 110): пачки наблюдателя — в свой поток, чтобы
/// поток наблюдателя не ждал ни индекса, ни очереди планов, ни записи ввода
/// окна.
///
/// Поток берёт пачку (подтверждённые внешние переименования заметок,
/// [`relink::external::confirmed`]) и:
/// 1. ждёт, пока индекс сверен ([`WindowIndex::ready_for_relink`](super::WindowIndex::ready_for_relink));
///    пачки, пришедшие за это время, присоединяются к ней;
/// 2. занимает общую очередь планов хранилища ([`Relinker::serial`]) — там
///    же переименования приложения, повторы и доделка; присоединяет пачки,
///    пришедшие, пока очередь была занята;
/// 3. считает план по текущему списку заметок и индексу
///    ([`relink::external::plan`]); переписывать нечего — ничего не делает
///    (ни вопроса, ни записи ввода);
/// 4. «Всегда спрашивать» — одно `links:pending {reason: external_rename}`
///    на пачку со знаком ([`Grant::External`]), ответ — `relink_answer`;
///    «Всегда обновлять» — сразу [`apply_external`].
///
/// Пачка отправляется под замком списка заметок, в котором уже применена
/// (`VaultEvents`): список, который видит поток, никогда не опережает его
/// очередь пачек. Поток выходит, когда закрыто окно (наблюдатель
/// остановлен — отправителя больше нет — или хранилище окна освобождено).
pub struct ExternalRenames {
    batches: mpsc::Sender<Vec<PathRename>>,
}

impl ExternalRenames {
    /// Поток внешних переименований хранилища `open`; события — окну `emit`.
    pub fn start(open: Weak<OpenVault>, emit: Arc<dyn Emit>) -> Self {
        let (batches, inbox) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("rhizomind-external-renames".into())
            .spawn(move || external_loop(&inbox, &open, &*emit));
        Self { batches }
    }

    /// Пачка внешних переименований (по порядку пачки наблюдателя).
    pub fn submit(&self, renames: Vec<PathRename>) {
        if !renames.is_empty() {
            // Поток вышел — окно закрыто, пачка никому не нужна.
            let _ = self.batches.send(renames);
        }
    }
}

fn external_loop(inbox: &mpsc::Receiver<Vec<PathRename>>, open: &Weak<OpenVault>, emit: &dyn Emit) {
    while let Ok(mut renames) = inbox.recv() {
        let open = loop {
            let Some(open) = open.upgrade() else {
                return;
            };
            if open.index.ready_for_relink() {
                break open;
            }
            drop(open);
            match inbox.recv_timeout(READY_POLL) {
                Ok(more) => renames.extend(more),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        };
        on_external(&open, emit, renames, inbox);
    }
}

/// Пачка внешних переименований при готовом индексе: вопрос или сразу
/// обновление — по настройке `rename_links`.
fn on_external(
    open: &OpenVault,
    emit: &dyn Emit,
    mut renames: Vec<PathRename>,
    inbox: &mpsc::Receiver<Vec<PathRename>>,
) {
    let _serial = open.relink.serial();
    renames.extend(inbox.try_iter().flatten());
    let (plan, renames) = external_plan(open, &renames);
    if plan.notes.is_empty() {
        return;
    }
    // Настройка не читается — спросить: без согласия ничего не переписать.
    let mode = open
        .vault
        .read_settings()
        .map_or(RenameLinks::Ask, |settings| settings.rename_links);
    match mode {
        RenameLinks::Update => apply_external(open, emit, &renames, None),
        RenameLinks::Ask => {
            let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
            let Ok(token) = open.relink.issue(Grant::External(renames.clone())) else {
                return;
            };
            send(
                emit,
                event::LINKS_PENDING,
                &LinksPendingPayload {
                    token,
                    reason: PendingReason::ExternalRename,
                    notes: count(plan.note_count()),
                    links: count(plan.link_count()),
                    renames: renames
                        .iter()
                        .take(limits::MAX_PENDING_RENAMES)
                        .cloned()
                        .collect(),
                    renames_total: count(renames.len()),
                },
            );
        }
    }
}

/// План для внешних переименований `renames` по списку заметок и индексу
/// сейчас (замки «список заметок → индекс», без чтения диска) и
/// переименования, по которым он посчитан.
fn external_plan(open: &OpenVault, renames: &[PathRename]) -> (RelinkPlan, Vec<PathRename>) {
    let list = open.read_notes();
    let now = list.note_refs();
    open.index
        .with_index(|index| relink::external::plan(index, &now, renames))
}

/// Обновление ссылок после внешних переименований (очередь планов уже
/// занята): запись ввода окна → план заново (записанный ввод мог добавить
/// ссылки) → переписывание → `links:updated` (`external_rename`).
///
/// Инвариант 4: ввод не записан (отказ или нет ответа за
/// [`FLUSH_TIMEOUT`]) — ни один файл не переписан. Отменить
/// переименование нельзя — оно уже на диске, — поэтому `links:updated` со
/// сбоем `busy` у каждой заметки плана и знаком «Повторить».
fn apply_external(
    open: &OpenVault,
    emit: &dyn Emit,
    renames: &[PathRename],
    token: Option<RelinkToken>,
) {
    // Переписывать уже нечего (например, переименовали обратно) — и ввод
    // записывать незачем.
    let (plan, kept) = external_plan(open, renames);
    if plan.notes.is_empty() {
        return;
    }
    if !open.relink.flush(emit) {
        not_started(open, emit, plan, &kept, LinksUpdateReason::ExternalRename);
        return;
    }
    let (plan, kept) = external_plan(open, renames);
    if plan.notes.is_empty() {
        return;
    }
    execute(
        open,
        emit,
        &plan,
        &kept,
        LinksUpdateReason::ExternalRename,
        token,
    );
}

/// Доделка обновления ссылок, прерванного при прошлом запуске (0.2-4.5,
/// [`relink::resume_on_open`]). Вызывается потоком индекса **после сверки
/// индекса** и до признака `reconciled` — то есть до любого другого плана.
/// Журнала нет — ничего не делает. Итог — `links:updated` с `reason:
/// resumed`, `note:changed` по переписанным заметкам, индекс их перечитывает.
pub fn resume_after_reconcile(open: &OpenVault, emit: &dyn Emit) {
    let _serial = open.relink.serial();
    if !relink::journal_pending(&open.vault) {
        return;
    }
    let notes = current_links(open);
    let mut progress = ProgressEvents::new(emit, None);
    let Some(report) = relink::resume_on_open(&open.vault, &notes, &mut |done, total| {
        progress.on(done, total, Instant::now())
    }) else {
        return;
    };
    if report.updated.is_empty() && report.failed.is_empty() && report.unmatched.is_empty() {
        return;
    }
    publish(open, emit, &report, &[], LinksUpdateReason::Resumed);
}

/// Отправитель событий окну вызова.
fn emitter<R: Runtime>(window: &Window<R>) -> WindowEmitter<R> {
    WindowEmitter::new(window.app_handle().clone(), window.label())
}

/// Сколько заметок и ссылок перепишет переименование или перемещение, и
/// знак плана. Пока индекс строится или ещё не сверен с диском —
/// `index_building`: интерфейс показывает окно «Подождите, индекс
/// строится…» в любом режиме настройки (решение 114) и спрашивает снова
/// по `index:status` (после сверки оно приходит ещё раз).
#[tauri::command]
pub async fn relink_preview<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: RelinkPreviewRequest,
) -> CoreResult<IndexReply<RelinkPreview>> {
    check_preview(&request)?;
    let open = core.require_vault(window.label())?;
    blocking(move || {
        if !open.index.ready_for_relink() {
            let status = open.index.status();
            return Ok(IndexReply::IndexBuilding(IndexProgress {
                done: status.done,
                total: status.total,
            }));
        }
        let rename = request.rename();
        let plan = plan_for(&open, &rename);
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let token = open.relink.issue(Grant::Preview(rename))?;
        Ok(IndexReply::Ready(RelinkPreview {
            notes: count(plan.note_count()),
            links: count(plan.link_count()),
            token,
        }))
    })
    .await
}

/// Ответ пользователя на `links:pending` или «Повторить». Итог приходит
/// событием `links:updated`. Повтор и «Обновить ссылки» после внешних
/// переименований — та же очередь и запись ввода, что у переименования;
/// план внешних переименований считается заново, ссылки снова находятся и
/// проверяются на свежем тексте.
/// `relink_answer` в потоке для блокирующих действий: знак окна `window`
/// → «Не обновлять» (ничего) или очередь планов, запись ввода и
/// переписывание. Знак предпросмотра — `invalid_argument`: он только для
/// `entry_rename`/`entry_move`.
pub fn relink_answer_blocking(
    open: &OpenVault,
    emit: &dyn Emit,
    window: &str,
    request: RelinkAnswerRequest,
) -> CoreResult<()> {
    let grant = open.relink.take(window, &request.token, "token")?;
    if matches!(grant, Grant::Preview(_)) {
        return Err(CoreError::invalid_argument("token"));
    }
    if !request.apply {
        return Ok(());
    }
    let _serial = open.relink.serial();
    match grant {
        Grant::Retry { plan, renames } => {
            if open.relink.flush(emit) {
                execute(
                    open,
                    emit,
                    &plan,
                    &renames,
                    LinksUpdateReason::Retry,
                    Some(request.token),
                );
            } else {
                not_started(open, emit, plan, &renames, LinksUpdateReason::Retry);
            }
        }
        Grant::External(renames) => apply_external(open, emit, &renames, Some(request.token)),
        Grant::Preview(_) => {}
    }
    Ok(())
}

#[tauri::command]
pub async fn relink_answer<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: RelinkAnswerRequest,
) -> CoreResult<()> {
    limits::token(&request.token, "token")?;
    let open = core.require_vault(window.label())?;
    let emit = emitter(&window);
    let label = window.label().to_owned();
    blocking(move || relink_answer_blocking(&open, &emit, &label, request)).await
}

/// Окно записало (или не смогло записать) несохранённый ввод перед
/// переписыванием ссылок.
#[tauri::command]
pub async fn links_flush_done<R: Runtime>(
    window: Window<R>,
    core: State<'_, Core>,
    request: LinksFlushDoneRequest,
) -> CoreResult<()> {
    limits::token(&request.token, "token")?;
    let open = core.require_vault(window.label())?;
    if open.relink.flush_done(&request.token, request.saved) {
        Ok(())
    } else {
        Err(CoreError::invalid_argument("token"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<(String, serde_json::Value)>>);

    impl Emit for Recorder {
        fn emit(&self, event: &str, payload: serde_json::Value) {
            self.0
                .lock()
                .expect("журнал")
                .push((event.to_owned(), payload));
        }
    }

    /// `links:progress` — не чаще 4 раз в секунду, сколько бы заметок ни
    /// прошло.
    #[test]
    fn progress_at_most_four_per_second() {
        let recorder = Recorder::default();
        let mut progress = ProgressEvents::new(&recorder, None);
        let start = Instant::now();
        for i in 0..1_000 {
            progress.on(i, 1_000, start + Duration::from_millis(i as u64));
        }
        let events = recorder.0.lock().expect("журнал");
        assert_eq!(events.len(), 4, "{events:?}");
        assert!(events.iter().all(|(name, _)| name == event::LINKS_PROGRESS));
    }

    #[test]
    fn preview_paths() {
        let path = VaultPath::from_components(["Альфа", "Встреча.md"]);
        assert_eq!(
            RelinkPreviewRequest::Rename {
                path: path.clone(),
                new_name: "Планёрка.md".into()
            }
            .rename()
            .to,
            VaultPath::from_components(["Альфа", "Планёрка.md"])
        );
        assert_eq!(
            RelinkPreviewRequest::Move {
                path,
                new_dir: VaultPath::root()
            }
            .rename()
            .to,
            VaultPath::from_components(["Встреча.md"])
        );
    }

    #[test]
    fn flush_answer_unknown_token_rejected() {
        let relinker = Relinker::default();
        assert!(!relinker.flush_done(&RelinkToken("a".repeat(32)), true));
    }

    #[test]
    fn flush_times_out_without_answer() {
        let relinker = Relinker::default();
        relinker.set_flush_timeout(Duration::from_millis(50));
        let recorder = Recorder::default();
        assert!(!relinker.flush(&recorder));
        let events = recorder.0.lock().expect("журнал");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, event::LINKS_FLUSH_REQUESTED);
        let token = RelinkToken(events[0].1["token"].as_str().expect("знак").to_owned());
        assert!(
            !relinker.flush_done(&token, true),
            "поздний ответ не принят"
        );
    }
}
