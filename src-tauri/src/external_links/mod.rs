//! Внешние ссылки (задача 1.5).
//!
//! Спецификация 0.1, С3.7 и ADR 0009: только схемы `http`, `https`,
//! `mailto`; окно подтверждения с полным адресом показывает ядро
//! (`tauri-plugin-dialog` только из Rust), по умолчанию выбрана кнопка
//! «Открыть в браузере»; после согласия адрес передаётся ОС (`os.rs`:
//! `ShellExecuteExW` с классом протокола на Windows, крейт `open` на macOS и Linux).
//! У интерфейса нет ни прав `opener:*`, ни другой команды открытия, поэтому
//! обойти подтверждение он не может.
//!
//! Шов для тестов — [`open_checked`]: «показать окно подтверждения» и
//! «передать адрес ОС» передаются замыканиями.

mod address;
mod os;

use std::collections::BTreeSet;
use std::io;
use std::sync::{Mutex, PoisonError};

pub use address::ExternalAddress;

use crate::commands::{CoreError, CoreResult, ErrorCode, OpenExternalOutcome};

/// Схемы, которые можно открыть.
pub const ALLOWED_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

/// Содержимое окна подтверждения (тексты — макет Э6б).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmation {
    /// Заголовок окна.
    pub title: String,
    /// Текст окна; полный адрес — отдельной строкой, ровно как уйдёт в ОС.
    pub message: String,
    /// Кнопка согласия — первая, выбрана по умолчанию.
    pub open_label: String,
    /// Кнопка отказа.
    pub cancel_label: String,
}

impl Confirmation {
    /// Окно подтверждения для проверенного адреса.
    pub fn for_address(address: &ExternalAddress) -> Self {
        let mut message = String::new();
        if address.is_mail() {
            message.push_str(
                "Открыть ссылку в почтовой программе?\n\n\
                 Ссылка из заметки откроется в почтовой программе, которая \
                 установлена в системе. Сам Rhizomind в сеть не выходит.\n\n",
            );
        } else {
            message.push_str(
                "Открыть ссылку в браузере?\n\n\
                 Ссылка из заметки откроется в браузере, который установлен \
                 в системе. Сам Rhizomind в сеть не выходит.\n\n",
            );
        }
        if let Some(host) = address.host_line() {
            message.push_str("Сайт: ");
            message.push_str(&host);
            message.push_str("\n\n");
        }
        message.push_str("Полный адрес:\n");
        message.push_str(address.as_str());
        Self {
            title: "Rhizomind — внешняя ссылка".into(),
            message,
            open_label: if address.is_mail() {
                "Открыть в почте".into()
            } else {
                "Открыть в браузере".into()
            },
            cancel_label: "Отмена".into(),
        }
    }
}

/// Проверяет адрес: схема (`scheme_not_allowed`), скрытые символы, длина,
/// вложения `mailto` (`invalid_argument`). Подробности — [`ExternalAddress::parse`].
pub fn check_url(url: &str) -> CoreResult<()> {
    ExternalAddress::parse(url).map(|_| ())
}

/// Окна, в которых сейчас открыто окно подтверждения ядра.
static DIALOG_WINDOWS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Занятость окна диалогом ядра; снимается при выходе из области видимости.
struct DialogSlot(String);

impl DialogSlot {
    /// `None`, если в окне уже открыт диалог ядра.
    fn take(window: &str) -> Option<Self> {
        let mut busy = DIALOG_WINDOWS
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        busy.insert(window.to_owned())
            .then(|| Self(window.to_owned()))
    }
}

impl Drop for DialogSlot {
    fn drop(&mut self) {
        DIALOG_WINDOWS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.0);
    }
}

/// Проверяет адрес, спрашивает пользователя через `ask` и только после
/// согласия передаёт ОС через `launch` ровно ту строку, что была в окне.
///
/// В одном окне — не больше одного окна подтверждения: пока первое открыто,
/// второй вызов сразу получает `cancelled` (окно не показывается, адрес в ОС
/// не уходит).
pub(crate) fn open_checked(
    window: &str,
    url: &str,
    ask: impl FnOnce(&Confirmation) -> bool,
    launch: impl FnOnce(&str) -> io::Result<()>,
) -> CoreResult<OpenExternalOutcome> {
    let address = ExternalAddress::parse(url)?;
    let Some(_slot) = DialogSlot::take(window) else {
        return Ok(OpenExternalOutcome::Cancelled);
    };
    if !ask(&Confirmation::for_address(&address)) {
        return Ok(OpenExternalOutcome::Cancelled);
    }
    launch(address.as_str()).map_err(|_| CoreError::new(ErrorCode::IoFailed))?;
    Ok(OpenExternalOutcome::Opened)
}

/// Проверяет адрес, показывает окно подтверждения ядра — модально к окну
/// `window`, из которого пришла ссылка, — и после согласия передаёт адрес
/// ОС. Блокирует поток до ответа пользователя: вызывать вне рабочих потоков
/// async (`spawn_blocking`) и не из главного потока. Длина и управляющие
/// символы уже проверены слоем команд (`commands::limits::url`).
pub fn open_with_confirmation<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    url: &str,
) -> CoreResult<OpenExternalOutcome> {
    open_checked(
        window.label(),
        url,
        |confirmation| ask(window, confirmation),
        os::launch,
    )
}

/// В программе — системное окно подтверждения.
#[cfg(not(test))]
fn ask<R: tauri::Runtime>(window: &tauri::Window<R>, confirmation: &Confirmation) -> bool {
    ask_in_dialog(window, confirmation)
}

/// В тестах крейта (подменный runtime) системное окно не показывается и
/// браузер не открывается: ответ всегда «отмена». Тесты прав команд вызывают
/// `open_external` с правильным адресом; без этого они ждали бы окна вечно,
/// а на Windows показали бы настоящее окно. Логика проверяется на шве
/// [`open_checked`].
#[cfg(test)]
fn ask<R: tauri::Runtime>(_window: &tauri::Window<R>, _confirmation: &Confirmation) -> bool {
    false
}

/// Системное окно подтверждения `tauri-plugin-dialog`. Кнопка согласия —
/// первая: в Windows (TaskDialog, `nDefaultButton = 0`) и macOS (NSAlert)
/// первая кнопка выбрана по умолчанию, Enter открывает, Esc — отмена.
/// Если окно показать не удалось — ответ «отмена».
#[cfg_attr(test, allow(dead_code))]
fn ask_in_dialog<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    confirmation: &Confirmation,
) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    let (answer, reply) = std::sync::mpsc::sync_channel(1);
    window
        .dialog()
        .message(&confirmation.message)
        .title(&confirmation.title)
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            confirmation.open_label.clone(),
            confirmation.cancel_label.clone(),
        ))
        .parent(window)
        .show(move |confirmed| {
            let _ = answer.send(confirmed);
        });
    reply.recv().unwrap_or(false)
}

#[cfg(test)]
mod tests;
