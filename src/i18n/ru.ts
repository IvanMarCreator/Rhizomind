// Словарь интерфейса `ru` (С3.8): все тексты интерфейса по ключам.
// Термины — по CONTEXT.md. Параметры — `{имя}`. Ключи ошибок ядра —
// `errors.<code>`, причины отказа в имени — `names.<problem>` (src/api/types.ts);
// их полноту проверяет completeness.test.ts.
export const ru = {
  'app.name': 'Rhizomind',

  // Лента иконок
  'shell.ribbon.label': 'Лента иконок',
  'shell.ribbon.files': 'Файлы',
  'shell.ribbon.quickSwitcher': 'Быстрый переход (Ctrl+O)',
  'shell.ribbon.settings': 'Настройки',

  // Боковая панель
  'shell.sidebar.label': 'Боковая панель',
  'shell.sidebar.files': 'Файлы',

  // Переключатель хранилищ
  'vaults.switcher.current': 'Хранилище «{name}»',
  'vaults.switcher.none': 'Хранилище не открыто',
  'vaults.switcher.open': 'Сменить хранилище',

  // Строка над заметкой
  'note.header.label': 'Заметка',
  'note.header.back': 'Назад (Alt+←)',
  'note.header.forward': 'Вперёд (Alt+→)',
  'note.header.path': 'Путь к заметке',
  'note.mode.label': 'Режим редактора',
  'note.mode.livePreview': 'Живой предпросмотр',
  'note.mode.source': 'Исходный текст (Ctrl+E)',

  // Редактор: текст и сохранение
  'editor.label': 'Текст заметки',
  'editor.loading': 'Открываем заметку…',
  'editor.openFailed': 'Не удалось открыть заметку. {message}',
  'editor.status.label': 'Состояние сохранения',
  'editor.status.saved': 'Сохранено',
  'editor.status.pending': 'Сохраняется',
  'editor.status.failed': 'Не сохранено',
  'editor.status.readOnly': 'Только чтение',
  'editor.status.deleted': 'Удалена на диске',
  'editor.status.missing': 'Не сохранено: файла нет на диске',
  'editor.closeUnsaved': 'Ввод в заметке «{name}» не сохранён: {message}',
  'editor.saveFailed':
    'Не удалось сохранить заметку «{name}». {message} Текст остаётся в редакторе, сохранение повторится.',
  'editor.conflictSaved':
    'Заметка «{name}» изменилась на диске одновременно с вашими правками. Сохранены обе версии: ваша — в заметке, версия с диска — рядом, в «{copy}».',
  'editor.readOnly.tooLarge.title': 'Только чтение: файл {size}, лимит для редактирования — {limit}',
  'editor.readOnly.tooLarge.hint':
    'Чтобы редактировать, увеличьте «Максимальный размер файла для редактирования» в настройках хранилища и откройте файл заново.',
  'editor.readOnly.tooLarge.hidden': 'Файл больше {max}, поэтому его текст не показывается.',
  'editor.readOnly.invalidUtf8.title': 'Только чтение: файл не в кодировке UTF-8',
  'editor.readOnly.invalidUtf8.hint':
    'Неверные байты показаны знаком «�». Правка запрещена, чтобы не испортить файл.',
  'editor.readOnly.openSettings': 'Открыть настройки',
  'units.megabytes': '{value} МБ',

  // Пустое состояние
  'shell.empty.title': 'Нет открытой заметки',
  'shell.empty.hint': 'Выберите заметку в дереве файлов или откройте быстрый переход: Ctrl+O.',

  // Уведомления внизу окна
  'notifications.label': 'Уведомления',
  'notifications.close': 'Закрыть уведомление',
  'layout.recovered': 'Раскладка окна была повреждена и сброшена.',

  // Запуск окна
  'startup.loading': 'Открываем окно…',
  'startup.apiMismatch.title': 'Части приложения не совпадают по версии',
  'startup.apiMismatch.body':
    'Интерфейс ожидает API ядра версии {expected}, а ядро сообщает версию {actual}. Установите Rhizomind заново из одного выпуска.',
  'startup.failed.title': 'Окно не открылось',

  // Ошибки команд ядра: errors.<code>
  'errors.not_implemented': 'Эта функция ещё не готова.',
  'errors.invalid_argument': 'Ядро отклонило неверные данные запроса.',
  'errors.no_vault_open': 'В этом окне не открыто хранилище.',
  'errors.vault_not_found': 'Хранилища нет в списке хранилищ.',
  'errors.vault_unavailable': 'Папка хранилища недоступна. Проверьте, подключён ли диск.',
  'errors.invalid_path': 'Неверный путь в хранилище.',
  'errors.outside_vault': 'Путь ведёт за пределы хранилища.',
  'errors.not_found': 'Файл или папка не найдены.',
  'errors.already_exists': 'Файл или папка с таким именем уже есть.',
  'errors.invalid_name': 'Такое имя использовать нельзя.',
  'errors.not_a_note': 'Это не заметка: открываются только файлы .md.',
  'errors.read_only': 'Заметка открыта только для чтения.',
  'errors.scheme_not_allowed': 'Ссылки такого вида не открываются.',
  'errors.io_failed': 'Не удалось прочитать или записать файл.',
  'errors.ipc_failed': 'Внутренняя ошибка приложения. Перезапустите окно.',

  // Причины отказа в имени: names.<problem>
  'names.empty': 'Введите имя.',
  'names.forbidden_chars': 'В имени нельзя использовать символы \\ / : * ? " < > |',
  'names.reserved_name': 'Это имя зарезервировано в Windows (CON, PRN, AUX, NUL, COM1, LPT1 и подобные).',
  'names.trailing_dot_or_space': 'Имя не может заканчиваться точкой или пробелом.',
  'names.too_long': 'Имя слишком длинное.',
  'names.duplicate': 'В этой папке уже есть элемент с таким именем.',
  'names.note_extension': 'Расширение .md нельзя убрать у заметки или добавить другому файлу.',

  // Предупреждения о допустимом имени: names.warning.<warning>
  'names.warning.unlinkable': 'На эту заметку нельзя будет сослаться.',
  'names.warning.hidden': 'Имя с точкой в начале: элемент будет скрыт в дереве файлов.',
} as const satisfies Record<string, string>;
