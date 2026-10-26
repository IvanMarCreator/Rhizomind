// Единственный список команд ядра API v1 (спецификация 0.1, С5.5).
//
// Файл подключается дважды: в `build.rs` (через `include!`) — для
// `AppManifest::commands`, и в ядро — для `generate_handler!`. Поэтому список
// прав и список обработчиков не могут разойтись: оба строятся из одного
// макроса. Новая команда добавляется сюда, в модуль `commands`, в файл
// разрешений `capabilities/main.json` и в снимки `contracts/v1/` (ADR 0009).
//
// Файл не должен ничего импортировать: в `build.rs` он компилируется отдельно.

/// Передаёт список команд ядра другому макросу: `with_core_commands!(m)`
/// раскрывается в `m!(vault_list, vault_current, ...)`.
macro_rules! with_core_commands {
    ($callback:ident) => {
        $callback!(
            // Хранилища
            vault_list,
            vault_current,
            vault_create,
            vault_open_folder,
            vault_open,
            vault_remove_from_list,
            vault_manager_open,
            folder_pick,
            // Файлы
            tree_list,
            note_create,
            folder_create,
            entry_rename,
            entry_move,
            entry_trash,
            name_check,
            // Заметка
            note_read,
            note_write,
            // Ссылки и поиск
            links_resolve,
            links_suggest,
            switcher_search,
            // Настройки и раскладка
            settings_read,
            settings_write,
            workspace_read,
            workspace_write,
            // Внешние ссылки
            open_external,
            // Окно, задача 0.7
            window_close_ready,
            window_close_cancel
        )
    };
}

/// Имена команд строками — для `AppManifest::commands` и тестов.
macro_rules! core_command_names {
    ($($command:ident),* $(,)?) => {
        &[$(stringify!($command)),*]
    };
}
