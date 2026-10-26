// Состояние окна: запуск (версия API, настройки, раскладка), открытая заметка,
// история переходов, уведомления. Компоненты оболочки только рисуют его;
// другие части интерфейса (дерево 2.3, быстрый переход 2.7) открывают
// заметки через `open()`. Для дерева файлов окно — TreeHost: оно узнаёт о
// переименовании и удалении, сделанных в приложении.
import { ApiVersionMismatch, checkApiVersion, core } from '../api/index.ts';
import type { CurrentVault, EditorMode, HotkeyOverrides, Settings, Theme, VaultPath } from '../api/index.ts';
import {
  activePane,
  closeNote,
  defaultLayout,
  openNote,
  parseLayout,
  serializeLayout,
  type Layout,
} from '../core/layout/layout.ts';
import { NavigationHistory, type HistoryAction } from '../core/history/history.ts';
import { userMessage } from '../i18n/errors.ts';
import { t, type MessageKey } from '../i18n/index.ts';
import { DEFAULT_SETTINGS } from '../settings/defaults.ts';
import type { TreeHost } from '../tree/tree.svelte.ts';

export type WindowStatus =
  | { kind: 'loading' }
  | { kind: 'ready' }
  /** Окно не может работать: заголовок — ключ словаря, текст уже переведён. */
  | { kind: 'failed'; title: MessageKey; message: string };

export interface Notice {
  id: number;
  text: string;
}

/** `path` — это `prefix` или лежит внутри него. */
function isInside(path: VaultPath, prefix: VaultPath): boolean {
  return prefix.length <= path.length && prefix.every((part, i) => part === path[i]);
}

export class WindowModel implements TreeHost {
  status = $state<WindowStatus>({ kind: 'loading' });
  vault = $state<CurrentVault | null>(null);
  /** Настройки хранилища (С3.1); меняются через `updateSettings`. */
  settings = $state.raw<Settings>(DEFAULT_SETTINGS);
  theme = $state<Theme>(DEFAULT_SETTINGS.theme);
  editorMode = $state<EditorMode>(DEFAULT_SETTINGS.editor_mode);
  /** Настройка хранилища «подтверждение удаления» (С3.1). */
  confirmDelete = $state(DEFAULT_SETTINGS.confirm_delete);
  layout = $state.raw<Layout>(defaultLayout());
  notices = $state<Notice[]>([]);
  sidebarOpen = $state(true);
  canGoBack = $state(false);
  canGoForward = $state(false);

  readonly #history = new NavigationHistory();
  #nextNotice = 1;
  /** Последние записанные в хранилище настройки: к ним откат, если запись не удалась. */
  #savedSettings: Settings = DEFAULT_SETTINGS;
  /** Записи настроек идут по очереди: последняя не обгоняет предыдущую. */
  #settingsWrite: Promise<void> = Promise.resolve();

  /** Режим заметки выбран в этом окне (кнопкой или Ctrl+E): чтение настроек его не перебивает. */
  #modeChosen = false;

  /** Назначения горячих клавиш хранилища (src/settings/hotkeys.ts). */
  get hotkeys(): HotkeyOverrides {
    return this.settings.hotkeys;
  }

  /** Путь заметки в активной вкладке; `null` — заметка не открыта. */
  get currentPath(): VaultPath | null {
    const pane = activePane(this.layout);
    return pane.tabs[pane.active]?.path ?? null;
  }

  /**
   * Запуск окна: первая команда — сверка версии API. Другая версия или отказ
   * моста — окно с сообщением. Настройки и раскладка не обязательны: при
   * ошибке окно работает со значениями по умолчанию и показывает уведомление.
   */
  async start(): Promise<void> {
    try {
      const current = await checkApiVersion();
      this.vault = current.vault;
      if (current.vault) await Promise.all([this.#loadSettings(), this.#loadLayout()]);
      this.status = { kind: 'ready' };
    } catch (error) {
      const title = error instanceof ApiVersionMismatch ? 'startup.apiMismatch.title' : 'startup.failed.title';
      this.status = { kind: 'failed', title, message: userMessage(error) };
    }
  }

  /** Открыть заметку: новый шаг истории, раскладка сохраняется в хранилище. */
  open(path: VaultPath): void {
    this.#history.visit(path);
    this.#show(path);
  }

  navigate(action: HistoryAction): void {
    const path = action === 'back' ? this.#history.back() : this.#history.forward();
    if (path) this.#show(path);
  }

  back(): void {
    this.navigate('back');
  }

  forward(): void {
    this.navigate('forward');
  }

  /** Переименование или перемещение в приложении: история и открытая заметка следуют за путём. */
  entryRenamed(from: VaultPath, to: VaultPath): void {
    this.#history.rename(from, to);
    const path = this.currentPath;
    if (path && isInside(path, from)) {
      this.layout = openNote(this.layout, [...to, ...path.slice(from.length)]);
      void this.#saveLayout();
    }
    this.#syncHistory();
  }

  /** Удаление в приложении: шаги истории уходят, открытая заметка внутри закрывается. */
  entryRemoved(path: VaultPath): void {
    this.#history.remove(path);
    const current = this.currentPath;
    if (current && isInside(current, path)) {
      this.layout = closeNote(this.layout);
      void this.#saveLayout();
    }
    this.#syncHistory();
  }

  /**
   * Режим редактора (решение 72): кнопки «книга / карандаш» и Ctrl+E. Выбор
   * сразу действует в окне и запоминается в настройках хранилища — через
   * `updateSettings`: пишется только `editor_mode` поверх файла.
   */
  setEditorMode(mode: EditorMode): void {
    this.#modeChosen = true;
    if (this.editorMode === mode) return;
    this.editorMode = mode;
    if (this.vault) void this.updateSettings({ editor_mode: mode });
  }

  toggleEditorMode(): void {
    this.setEditorMode(this.editorMode === 'live_preview' ? 'source' : 'live_preview');
  }

  toggleSidebar(): void {
    this.sidebarOpen = !this.sidebarOpen;
  }

  notify(text: string): void {
    this.notices = [...this.notices, { id: this.#nextNotice++, text }];
  }

  dismiss(id: number): void {
    this.notices = this.notices.filter((notice) => notice.id !== id);
  }

  /**
   * Меняет настройки хранилища: действуют сразу, в `.rhizomind/settings.json`
   * записываются в фоне. Пишутся только поля `patch` поверх прочитанных с
   * диска: поля, которые меняет не окно настроек (режим заметки, задача
   * 2.5), не затираются. Не записалось — уведомление и откат к записанным.
   */
  updateSettings(patch: Partial<Settings>): Promise<void> {
    const shown: Settings = { ...this.settings, ...patch };
    this.#showSettings(shown, false);
    this.#settingsWrite = this.#settingsWrite.then(async () => {
      try {
        const { settings: disk } = await core.settingsRead();
        const next: Settings = { ...disk, ...patch };
        await core.settingsWrite({ settings: next });
        this.#savedSettings = next;
      } catch (error) {
        this.notify(userMessage(error));
        this.#revert(patch);
      }
    });
    return this.#settingsWrite;
  }

  /**
   * Запись `patch` не удалась: его поля возвращаются к записанным значениям —
   * кроме тех, что с тех пор поменяла следующая правка (её запись в очереди).
   */
  #revert(patch: Partial<Settings>): void {
    const back: Partial<Settings> = {};
    for (const key of Object.keys(patch) as (keyof Settings)[]) {
      if (JSON.stringify(this.settings[key]) === JSON.stringify(patch[key])) {
        Object.assign(back, { [key]: this.#savedSettings[key] });
      }
    }
    this.#showSettings({ ...this.settings, ...back }, false);
  }

  /** Показывает настройки; режим заметки — только при чтении (его меняет заметка, а не окно настроек). */
  #showSettings(settings: Settings, withMode = true): void {
    this.settings = settings;
    this.theme = settings.theme;
    if (withMode) this.editorMode = settings.editor_mode;
    this.confirmDelete = settings.confirm_delete;
  }

  async #loadSettings(): Promise<void> {
    try {
      const { settings } = await core.settingsRead();
      this.#savedSettings = settings;
      // Режим выбрали, пока настройки читались: он не откатывается к прочитанному.
      if (this.#modeChosen) {
        this.#showSettings({ ...settings, editor_mode: this.editorMode });
        if (settings.editor_mode !== this.editorMode) void this.updateSettings({ editor_mode: this.editorMode });
      } else {
        this.#showSettings(settings);
      }
    } catch (error) {
      this.notify(userMessage(error));
    }
  }

  async #loadLayout(): Promise<void> {
    try {
      const { workspace } = await core.workspaceRead();
      const { layout, recovered } = parseLayout(workspace);
      this.layout = layout;
      if (recovered) this.notify(t('layout.recovered'));
      const path = this.currentPath;
      if (path) this.#history.visit(path);
      this.#syncHistory();
    } catch (error) {
      this.notify(userMessage(error));
    }
  }

  #show(path: VaultPath): void {
    this.layout = openNote(this.layout, path);
    this.#syncHistory();
    void this.#saveLayout();
  }

  #syncHistory(): void {
    this.canGoBack = this.#history.canGoBack;
    this.canGoForward = this.#history.canGoForward;
  }

  async #saveLayout(): Promise<void> {
    if (!this.vault) return;
    try {
      await core.workspaceWrite({ workspace: serializeLayout(this.layout) });
    } catch (error) {
      this.notify(userMessage(error));
    }
  }
}
