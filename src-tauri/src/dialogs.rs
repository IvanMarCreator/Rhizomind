//! Диалоги ядра: общий замок и окна, которые показывает ядро, а не
//! интерфейс (ADR 0009: прав `dialog:*` у интерфейса нет).
//!
//! Диалогов ядра три: подтверждение внешней ссылки (`external_links`),
//! выбор папки хранилища ([`pick_folder`]) и подтверждение безвозвратного
//! удаления на диске без корзины ([`confirm_permanent_delete`]). У всех
//! один замок ([`DialogSlot`]): **в одном окне приложения — не больше одного
//! диалога ядра одновременно**, какого бы вида он ни был (ревью 0.4 и 1.5,
//! задача 1.7). Пока диалог открыт, следующий запрос из того же окна сразу
//! получает ответ «отмена» — окно не показывается, действие не выполняется.
//!
//! Диалоги блокируют поток до ответа пользователя: вызывать вне рабочих
//! потоков async (`spawn_blocking`) и не из главного потока.
//!
//! Окно выбора папки на Windows — своё ([`folder_windows`], задача 0.8,
//! решение владельца 56): путь к выбранной папке не попадает в перемещаемый
//! профиль. На Linux и macOS — `tauri-plugin-dialog`. Окна подтверждения —
//! `tauri-plugin-dialog` на всех ОС.
//!
//! В тестах крейта (подменный runtime) системные окна не показываются:
//! ответ берётся из [`test_answers`] по метке окна, по умолчанию — отмена.
//! В тестовой сборке (функция `webdriver`) окно выбора папки можно
//! заменить ответом из переменной окружения (`windows::e2e::PICK_FOLDER_ENV`).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use crate::commands::{CoreResult, EntryKind};

#[cfg(windows)]
mod folder_windows;

/// Окна, в которых сейчас открыт диалог ядра.
static BUSY_WINDOWS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Занятость окна диалогом ядра; снимается при выходе из области видимости
/// (в том числе при панике внутри диалога).
#[derive(Debug)]
pub struct DialogSlot(String);

impl DialogSlot {
    /// `None`, если в окне `window` уже открыт диалог ядра.
    pub fn take(window: &str) -> Option<Self> {
        let mut busy = BUSY_WINDOWS.lock().unwrap_or_else(PoisonError::into_inner);
        busy.insert(window.to_owned())
            .then(|| Self(window.to_owned()))
    }
}

impl Drop for DialogSlot {
    fn drop(&mut self) {
        BUSY_WINDOWS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.0);
    }
}

/// Цель системного диалога выбора папки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderPurpose {
    /// Родительская папка для нового хранилища.
    NewVaultParent,
    /// Существующая папка, которая станет хранилищем.
    ExistingVault,
}

impl FolderPurpose {
    /// Заголовок окна выбора папки.
    pub fn title(self) -> &'static str {
        match self {
            Self::NewVaultParent => "Где создать хранилище",
            Self::ExistingVault => "Открыть папку как хранилище",
        }
    }
}

/// Системный диалог выбора папки, модальный к окну `window`. `Ok(None)` —
/// пользователь отменил выбор или в окне уже открыт другой диалог ядра.
pub fn pick_folder<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    purpose: FolderPurpose,
) -> CoreResult<Option<PathBuf>> {
    let Some(_slot) = DialogSlot::take(window.label()) else {
        return Ok(None);
    };
    Ok(ask_folder(window, purpose))
}

/// Тексты окна подтверждения безвозвратного удаления (С3.2, С6: «на диске
/// без корзины — безвозвратно, после отдельного подтверждения»).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermanentDelete {
    /// Заголовок окна.
    pub title: String,
    /// Текст окна.
    pub message: String,
    /// Кнопка согласия.
    pub delete_label: String,
    /// Кнопка отказа.
    pub cancel_label: String,
}

impl PermanentDelete {
    /// Окно для элемента с именем `name` (как на диске) вида `kind`.
    pub fn for_entry(name: &str, kind: EntryKind) -> Self {
        let what = match kind {
            EntryKind::Folder => format!("Папка «{name}» и всё, что в ней, будут удалены"),
            EntryKind::Note => {
                let stem = crate::names::note_stem(name).unwrap_or(name);
                format!("Заметка «{stem}» будет удалена")
            }
            EntryKind::Attachment => format!("Файл «{name}» будет удалён"),
        };
        Self {
            title: "Rhizomind — удаление без корзины".into(),
            message: format!(
                "Удалить безвозвратно?\n\n\
                 На этом диске нет корзины (например, флешка или сетевая папка). \
                 {what} навсегда — восстановить из корзины не получится."
            ),
            delete_label: "Удалить безвозвратно".into(),
            cancel_label: "Отмена".into(),
        }
    }
}

/// Окно подтверждения безвозвратного удаления, модальное к окну `window`.
/// `false` — пользователь отказался, окно не удалось показать или в окне уже
/// открыт другой диалог ядра.
pub fn confirm_permanent_delete<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    name: &str,
    kind: EntryKind,
) -> bool {
    let Some(_slot) = DialogSlot::take(window.label()) else {
        return false;
    };
    ask_permanent_delete(window, &PermanentDelete::for_entry(name, kind))
}

/// Текст сообщения «окно закрыто без записи» (решение владельца 66).
pub fn closed_unsaved_text(vault_name: &str) -> String {
    format!("Хранилище «{vault_name}» недоступно. Окно закрыто, несохранённый ввод не записан.")
}

/// Системное сообщение: окно хранилища `label` закрыто, ввод не записан —
/// хранилище на внешнем носителе стало недоступно (решение владельца 66).
/// Ждёт, пока пользователь закроет сообщение: окно уничтожается после
/// (иначе выход программы убрал бы сообщение раньше, чем его прочли).
/// Блокирует поток — вызывать вне главного.
pub fn tell_closed_unsaved<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    label: &str,
    vault_name: &str,
) {
    let text = closed_unsaved_text(vault_name);
    show_closed_unsaved(app, label, &text);
}

#[cfg(all(not(test), not(feature = "webdriver")))]
fn show_closed_unsaved<R: tauri::Runtime>(app: &tauri::AppHandle<R>, _label: &str, text: &str) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    app.dialog()
        .message(text)
        .title("Rhizomind")
        .kind(MessageDialogKind::Warning)
        .blocking_show();
}

/// Тестовая сборка: сообщение — в stderr, без окна (сценарный тест не
/// нажимает «ОК» в системном окне).
#[cfg(all(not(test), feature = "webdriver"))]
fn show_closed_unsaved<R: tauri::Runtime>(_app: &tauri::AppHandle<R>, _label: &str, text: &str) {
    eprintln!("Rhizomind (тестовая сборка, сообщение): {text}");
}

#[cfg(test)]
fn show_closed_unsaved<R: tauri::Runtime>(_app: &tauri::AppHandle<R>, label: &str, text: &str) {
    test_answers::record_closed_unsaved(label, text);
}

#[cfg(all(not(test), windows))]
fn ask_folder<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    purpose: FolderPurpose,
) -> Option<PathBuf> {
    // Тестовая сборка: ответ окна выбора папки — из переменной окружения
    // сценарного теста (в выпуске этой ветки нет: release-binary.test.ts).
    #[cfg(feature = "webdriver")]
    if let Some(folder) = crate::windows::e2e::picked_folder() {
        return Some(folder);
    }
    // Владелец — окно вызова: окно выбора модально к нему.
    let owner = match window.hwnd() {
        Ok(hwnd) => Some(hwnd.0 as isize),
        Err(error) => {
            eprintln!("Rhizomind: окно выбора папки без владельца (нет HWND окна): {error}");
            None
        }
    };
    folder_windows::pick_folder(owner, purpose.title())
}

#[cfg(all(not(test), not(windows)))]
fn ask_folder<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    purpose: FolderPurpose,
) -> Option<PathBuf> {
    use tauri_plugin_dialog::DialogExt;
    // Тестовая сборка: ответ окна выбора папки — из переменной окружения
    // сценарного теста (в выпуске этой ветки нет: release-binary.test.ts).
    #[cfg(feature = "webdriver")]
    if let Some(folder) = crate::windows::e2e::picked_folder() {
        return Some(folder);
    }
    window
        .dialog()
        .file()
        .set_title(purpose.title())
        .set_parent(window)
        .blocking_pick_folder()
        .and_then(|path| path.into_path().ok())
}

#[cfg(not(test))]
fn ask_permanent_delete<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    texts: &PermanentDelete,
) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let (answer, reply) = std::sync::mpsc::sync_channel(1);
    window
        .dialog()
        .message(&texts.message)
        .title(&texts.title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            texts.delete_label.clone(),
            texts.cancel_label.clone(),
        ))
        .parent(window)
        .show(move |confirmed| {
            let _ = answer.send(confirmed);
        });
    reply.recv().unwrap_or(false)
}

#[cfg(test)]
fn ask_folder<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    _purpose: FolderPurpose,
) -> Option<PathBuf> {
    test_answers::get(window.label()).folder
}

#[cfg(test)]
fn ask_permanent_delete<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    _texts: &PermanentDelete,
) -> bool {
    let answers = test_answers::get(window.label());
    if let Some(hold) = answers.hold {
        std::thread::sleep(hold);
    }
    answers.confirm_delete
}

/// Ответы диалогов в тестах, по метке окна (тесты идут параллельно).
#[cfg(test)]
pub(crate) mod test_answers {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Mutex, PoisonError};
    use std::time::Duration;

    /// Что «выбрал» пользователь.
    #[derive(Debug, Clone, Default)]
    pub struct Answers {
        /// Выбранная папка; `None` — отмена.
        pub folder: Option<PathBuf>,
        /// Согласие на безвозвратное удаление.
        pub confirm_delete: bool,
        /// Сколько «держать» окно удаления открытым.
        pub hold: Option<Duration>,
    }

    static ANSWERS: Mutex<Option<HashMap<String, Answers>>> = Mutex::new(None);
    static CLOSED_UNSAVED: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

    /// Показанное сообщение «окно закрыто, ввод не записан» окна `label`.
    pub fn record_closed_unsaved(label: &str, text: &str) {
        CLOSED_UNSAVED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(HashMap::new)
            .insert(label.to_owned(), text.to_owned());
    }

    /// Сообщение «окно закрыто, ввод не записан» окна `label`, если было.
    pub fn closed_unsaved(label: &str) -> Option<String> {
        CLOSED_UNSAVED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(|all| all.get(label).cloned())
    }

    /// Задаёт ответы для окна `label`.
    pub fn set(label: &str, answers: Answers) {
        ANSWERS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(HashMap::new)
            .insert(label.to_owned(), answers);
    }

    /// Ответы для окна `label` (по умолчанию — отмена).
    pub fn get(label: &str) -> Answers {
        ANSWERS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(|all| all.get(label).cloned())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_is_one_per_window_and_shared_by_all_dialog_kinds() {
        let first = DialogSlot::take("dialogs-test-a").expect("свободно");
        assert!(
            DialogSlot::take("dialogs-test-a").is_none(),
            "второй диалог в том же окне"
        );
        assert!(
            DialogSlot::take("dialogs-test-b").is_some(),
            "другое окно свободно"
        );
        drop(first);
        assert!(DialogSlot::take("dialogs-test-a").is_some(), "освобождено");
    }

    #[test]
    fn permanent_delete_texts_name_the_entry_and_warn() {
        let note = PermanentDelete::for_entry("План.md", EntryKind::Note);
        assert!(
            note.message
                .contains("Заметка «План» будет удалена навсегда")
        );
        assert!(note.message.contains("нет корзины"));
        assert_eq!(note.delete_label, "Удалить безвозвратно");
        assert_eq!(note.cancel_label, "Отмена");
        let folder = PermanentDelete::for_entry("Архив", EntryKind::Folder);
        assert!(
            folder
                .message
                .contains("Папка «Архив» и всё, что в ней, будут удалены")
        );
        let file = PermanentDelete::for_entry("схема.png", EntryKind::Attachment);
        assert!(file.message.contains("Файл «схема.png» будет удалён"));
    }
}
