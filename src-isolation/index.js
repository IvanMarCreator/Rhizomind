// Изолированный iframe (isolation pattern Tauri, ADR 0009). Каждый вызов ядра
// из интерфейса проходит через эту функцию до шифрования и отправки в ядро.
// Чужой скрипт на странице интерфейса сюда не дотягивается: iframe живёт на
// другом источнике (origin) и в песочнице.
//
// Пропускаются только команды из списков ниже; остальные подменяются на
// заведомо несуществующую команду без параметров — ядро её отклоняет, и вызов
// в интерфейсе завершается ошибкой, а не зависает.
//
// Обычный скрипт, не модуль: на Windows ES-модули внутри isolation не работают.
'use strict';

(function () {
  // Свои команды ядра API v1 — ровно список src-tauri/src/commands/list.rs
  // (из него же AppManifest::commands и generate_handler!). Новая команда
  // добавляется туда, в capabilities/main.json и сюда; расхождение ловит
  // тест filter.test.ts.
  var OWN_COMMANDS = Object.freeze([
    'vault_list',
    'vault_current',
    'vault_create',
    'vault_open_folder',
    'vault_open',
    'vault_remove_from_list',
    'vault_manager_open',
    'tree_list',
    'note_create',
    'folder_create',
    'entry_rename',
    'entry_move',
    'entry_trash',
    'name_check',
    'note_read',
    'note_write',
    'links_resolve',
    'links_suggest',
    'switcher_search',
    'settings_read',
    'settings_write',
    'workspace_read',
    'workspace_write',
    'open_external',
  ]);

  // События ядра API v1 — ровно модуль `event` в src-tauri/src/commands/events.rs
  // (EVENT_NAMES). Подписка на другие события, в том числе встроенные события
  // Tauri (`tauri://window-created`, `tauri://webview-created` и др.), запрещена:
  // Tauri рассылает их всем окнам, и в них — метки других окон (ревью
  // безопасности 1.3). Расхождение со списком ядра ловит тест filter.test.ts.
  var CORE_EVENTS = Object.freeze([
    'vault:changed',
    'note:changed',
    'note:renamed',
    'note:deleted',
    'note:conflict-saved',
    'vault:availability',
  ]);

  // Служебные команды Tauri, без которых не работают события ядра (`listen`).
  var INTERNAL_COMMANDS = Object.freeze(['plugin:event|listen', 'plugin:event|unlisten']);

  var REJECTED = '__rhizomind_isolation_rejected__';

  function isAllowed(cmd) {
    return (
      typeof cmd === 'string' && (OWN_COMMANDS.indexOf(cmd) !== -1 || INTERNAL_COMMANDS.indexOf(cmd) !== -1)
    );
  }

  // Подписка на события — только с целью «окно с меткой» (WebviewWindow),
  // как у src/api/events.ts (getCurrentWebviewWindow().listen). Подписка без
  // цели (Any) получала бы события ВСЕХ окон: Tauri отдаёт событие, отправленное
  // окну хранилища (emit_to), и слушателям Any в других окнах. Метку чужого окна
  // интерфейс не знает: метки окон случайные (src-tauri/src/windows/mod.rs).
  function isAllowedListen(payload) {
    if (payload === null || typeof payload !== 'object') return false;
    if (typeof payload.event !== 'string' || CORE_EVENTS.indexOf(payload.event) === -1) return false;
    var target = payload.target;
    return (
      target !== null &&
      typeof target === 'object' &&
      target.kind === 'WebviewWindow' &&
      typeof target.label === 'string' &&
      target.label !== ''
    );
  }

  function filterIpcMessage(message) {
    if (
      message !== null &&
      typeof message === 'object' &&
      isAllowed(message.cmd) &&
      (message.cmd !== 'plugin:event|listen' || isAllowedListen(message.payload))
    ) {
      return message;
    }
    var source = message !== null && typeof message === 'object' ? message : {};
    return {
      cmd: REJECTED,
      callback: source.callback,
      error: source.error,
      payload: {},
      options: undefined,
    };
  }

  window.__TAURI_ISOLATION_HOOK__ = filterIpcMessage;
})();
