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
  // Свои команды ядра. Новая команда добавляется в четыре места: код ядра,
  // AppManifest::commands (src-tauri/build.rs), файл разрешений
  // (src-tauri/capabilities/main.json) и этот список.
  var OWN_COMMANDS = Object.freeze([]);

  // Служебные команды Tauri, без которых не работают события ядра (`listen`).
  var INTERNAL_COMMANDS = Object.freeze(['plugin:event|listen', 'plugin:event|unlisten']);

  var REJECTED = '__rhizomind_isolation_rejected__';

  function isAllowed(cmd) {
    return (
      typeof cmd === 'string' && (OWN_COMMANDS.indexOf(cmd) !== -1 || INTERNAL_COMMANDS.indexOf(cmd) !== -1)
    );
  }

  function filterIpcMessage(message) {
    if (message !== null && typeof message === 'object' && isAllowed(message.cmd)) {
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
