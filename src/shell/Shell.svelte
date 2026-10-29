<!--
  Оболочка окна по концепту К2 (docs/design/README.md, Э1): лента иконок,
  боковая панель, область заметки со строкой над ней, уведомления внизу.
  Окно без хранилища — окно «Управление хранилищами» (src/vaults/, Э11).
  Заголовок окна — системный; его текст ставит ядро: оболочка сообщает только,
  какая заметка показана (ShownNote, задача 2.12, решение 82).
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { onCoreEvent } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import { newNoteDir } from '../quick-switcher/create.ts';
  import QuickSwitcher from '../quick-switcher/QuickSwitcher.svelte';
  import { applyTextSettings } from '../settings/apply.ts';
  import { bindingsOf, chordText, matchCommand, windowCommandInText, type CommandId } from '../settings/hotkeys.ts';
  import SettingsDialog, { type SettingsSection } from '../settings/SettingsDialog.svelte';
  import { applyTheme } from '../theme/theme.ts';
  import Editor from '../editor/Editor.svelte';
  import SaveIndicator from '../editor/SaveIndicator.svelte';
  import type { SaveStatus } from '../editor/session.ts';
  import NoteTitle from '../tree/NoteTitle.svelte';
  import { FileTreeState, type TreeHost } from '../tree/tree.svelte.ts';
  import VaultManager from '../vaults/VaultManager.svelte';
  import EmptyState from './EmptyState.svelte';
  import NoteHeader from './NoteHeader.svelte';
  import Notifications from './Notifications.svelte';
  import Ribbon from './Ribbon.svelte';
  import Sidebar from './Sidebar.svelte';
  import StartupError from './StartupError.svelte';
  import { ExternalChanges } from '../sync/external.ts';
  import { WindowClosing, coreClosing } from './closing.ts';
  import { ShownNote } from './shownNote.ts';
  import { WindowModel } from './window.svelte.ts';

  let { model = new WindowModel() }: { model?: WindowModel } = $props();
  let saveStatus = $state.raw<SaveStatus | null>(null);

  /** Редактор: сигналы «переименовано» и «удалено» доходят до него раньше, чем окно сменит путь. */
  let editor: Editor | undefined = $state();

  /** Дерево файлов окна: им же переименовывается заметка из заголовка. */
  const files = createFiles();

  function createFiles(): FileTreeState {
    const owner = model;
    const host: TreeHost = {
      get currentPath() {
        return owner.currentPath;
      },
      get confirmDelete() {
        return owner.confirmDelete;
      },
      get hotkeys() {
        return owner.hotkeys;
      },
      open: (path) => owner.open(path),
      entryRenamed: (from, to) => {
        // Сначала редактор: смена пути после этого — не уход с заметки.
        editor?.renamed(from, to);
        owner.entryRenamed(from, to);
      },
      entryRemoved: (path) => {
        // Ввод удалённой заметки не пишется (решение владельца).
        editor?.removed(path);
        owner.entryRemoved(path);
      },
      notify: (text) => owner.notify(text),
    };
    return new FileTreeState(host);
  }

  /**
   * Изменения файлов снаружи (задача 2.9, С3.6): события ядра `note:*` и
   * полная сверка — в редактор, историю и раскладку окна. Сначала редактор:
   * смена пути после этого — не уход с заметки.
   */
  const external = new ExternalChanges({
    get currentPath() {
      return model.currentPath;
    },
    changed: (path, version) => editor?.changed(path, version),
    renamed: (from, to) => {
      editor?.renamed(from, to);
      model.entryRenamed(from, to);
    },
    removed: (path) => {
      editor?.removed(path);
      model.entryRemoved(path);
    },
    rescanned: () => editor?.rescanned(),
    deletingHere: (path) => files.isDeleting(path),
    notify: (text) => model.notify(text),
  });

  /**
   * Закрытие окна (задача 0.7): ядро скрыло окно и ждёт, пока ввод запишется.
   * Без редактора (окно загружается, ошибка запуска) записывать нечего.
   */
  const closing = new WindowClosing({
    flush: () => editor?.flushForClose() ?? Promise.resolve([]),
    ...coreClosing,
    notify: (text) => model.notify(text),
  });

  /** Окно быстрого перехода (Ctrl+O, задача 2.7) открыто. */
  let switcherOpen = $state(false);
  /** Открытое окно «Настройки хранилища» и его раздел при открытии; `null` — закрыто. */
  let settingsSection = $state<SettingsSection | null>(null);

  /**
   * Открыто модальное окно (быстрый переход, подтверждение удаления,
   * настройки): горячие клавиши окна под ним не действуют.
   */
  const modalOpen = $derived(switcherOpen || settingsSection !== null || files.pendingDelete !== null);

  /** Первое сочетание команды для подсказок; `null` — у команды нет сочетаний. */
  function shortcutOf(id: CommandId): string | null {
    const chord = bindingsOf(model.hotkeys, id)[0];
    return chord ? chordText(chord) : null;
  }

  function openSwitcher() {
    if (model.status.kind === 'ready' && model.vault && !modalOpen) switcherOpen = true;
  }

  function openSettings(section: SettingsSection = 'general') {
    if (model.status.kind === 'ready' && model.vault && !modalOpen) settingsSection = section;
  }

  /** Новая заметка (Ctrl+N): папка — по настройке «где создавать новые заметки». */
  function createNote() {
    if (!model.sidebarOpen) model.toggleSidebar();
    void files.createNote(newNoteDir(model.settings.new_note_location, model.currentPath));
  }

  function run(command: CommandId) {
    switch (command) {
      case 'quick_switcher.open':
        openSwitcher();
        return;
      case 'note.new':
        createNote();
        return;
      case 'editor.toggle-mode':
        // Режим — у открытой заметки; запоминается в настройках хранилища.
        if (model.currentPath) model.toggleEditorMode();
        return;
      case 'history.back':
        model.back();
        return;
      case 'history.forward':
        model.forward();
        return;
      case 'tree.rename':
      case 'tree.delete':
        // Команды дерева: их обрабатывает дерево файлов (FileTree.svelte).
        return;
    }
  }

  onMount(() => {
    // Подписка — первой: окно могут закрыть, пока оно загружается.
    const unsubscribe = onCoreEvent('window:close-requested', (payload) => void closing.closeRequested(payload));
    const stopExternal = external.start();
    void model.start();
    // Не <svelte:window>: Svelte передаёт тогда объект window значением, и
    // проверка сети (tools/check-no-network) не может доказать безопасность.
    // Фаза перехвата: команды реестра важнее сочетаний редактора и полей
    // (решение 80), кроме клавиш правки текста в полях (решение 81).
    window.addEventListener('keydown', onKeydownCapture, true);
    window.addEventListener('keydown', onKeydownBubble);
    return () => {
      window.removeEventListener('keydown', onKeydownCapture, true);
      window.removeEventListener('keydown', onKeydownBubble);
      void unsubscribe.then((stop) => stop()).catch(() => undefined);
      stopExternal();
    };
  });

  $effect(() => applyTheme(model.theme));
  /** Заголовок окна (решение 82): ядру — путь показанной заметки; ввод текста путь не меняет. */
  const shownNote = new ShownNote();
  $effect(() => {
    if (model.status.kind === 'ready' && model.vault) shownNote.report(model.currentPath);
  });
  $effect(() => applyTextSettings(model.settings));

  /** Поле формы: поиск, быстрый переход, переименование, числа в настройках. */
  function isFormField(target: EventTarget | null): boolean {
    return target instanceof Element && target.closest('input, textarea, select') !== null;
  }

  /** Текст заметки (CodeMirror — contenteditable). */
  function isEditableText(target: EventTarget | null): boolean {
    return target instanceof Element && target.closest('[contenteditable]:not([contenteditable="false"])') !== null;
  }

  /**
   * Горячие клавиши окна — по реестру и назначениям хранилища
   * (src/settings/hotkeys.ts). Вне полей формы — в фазе перехвата: команды
   * важнее сочетаний редактора (решение 80), кроме клавиш правки текста в
   * тексте заметки (решение 81).
   */
  function onKeydownCapture(event: KeyboardEvent) {
    if (isFormField(event.target)) return;
    const command = isEditableText(event.target)
      ? windowCommandInText(model.hotkeys, event)
      : matchCommand(model.hotkeys, event, 'window');
    runCommand(event, command);
  }

  /**
   * Поля формы — после самого поля, на всплытии: поле переименования
   * останавливает нажатие у себя, и команды окна в нём не выполняются;
   * клавиши правки текста остаются полю (решение 81).
   */
  function onKeydownBubble(event: KeyboardEvent) {
    if (!isFormField(event.target) || event.defaultPrevented) return;
    runCommand(event, windowCommandInText(model.hotkeys, event));
  }

  function runCommand(event: KeyboardEvent, command: CommandId | null) {
    if (model.status.kind !== 'ready' || !model.vault || !command) return;
    event.preventDefault();
    // Под модальным окном команды окна не действуют.
    if (modalOpen) return;
    run(command);
  }
</script>

{#if model.status.kind === 'ready' && !model.vault}
  <!-- Окно без хранилища — окно «Управление хранилищами» (задача 2.8а). -->
  <VaultManager onNotice={(text) => model.notify(text)} />
{:else if model.status.kind === 'ready'}
  <div class="shell" data-testid="shell">
    <Ribbon
      filesOpen={model.sidebarOpen}
      onToggleFiles={() => model.toggleSidebar()}
      onQuickSwitcher={openSwitcher}
      quickSwitcherShortcut={shortcutOf('quick_switcher.open')}
      onSettings={() => openSettings()}
    />
    {#if model.sidebarOpen}
      <Sidebar vault={model.vault} {files} onNotice={(text) => model.notify(text)} />
    {/if}
    <main class="note">
      <NoteHeader
        path={model.currentPath}
        canGoBack={model.canGoBack}
        canGoForward={model.canGoForward}
        mode={model.editorMode}
        onMode={(mode) => model.setEditorMode(mode)}
        modeShortcut={bindingsOf(model.hotkeys, 'editor.toggle-mode')[0] ?? null}
        readOnly={saveStatus?.kind === 'read_only' || saveStatus?.kind === 'deleted'}
        onBack={() => model.back()}
        onForward={() => model.forward()}
      >
        {#snippet status()}
          <SaveIndicator status={saveStatus} />
        {/snippet}
      </NoteHeader>
      <div class="body">
        {#if !model.currentPath}
          <EmptyState />
        {:else}
          <!-- Заголовок над текстом; редактор (2.4) встаёт под ним в ту же колонку. -->
          <div class="column">
            <NoteTitle tree={files} path={model.currentPath} />
          </div>
        {/if}
        <Editor
          bind:this={editor}
          path={model.currentPath}
          mode={model.editorMode}
          showTitle={false}
          notify={(text) => model.notify(text)}
          onStatus={(status) => (saveStatus = status)}
          onOpenSettings={() => openSettings('files')}
          onOpenNote={(path) => model.open(path)}
        />
      </div>
    </main>
  </div>
  {#if switcherOpen}
    <QuickSwitcher
      currentPath={model.currentPath}
      onOpen={(path) => model.open(path)}
      onClose={() => (switcherOpen = false)}
      onNotice={(text) => model.notify(text)}
    />
  {/if}
  {#if settingsSection !== null && model.vault}
    <SettingsDialog
      vault={model.vault}
      settings={model.settings}
      section={settingsSection}
      onChange={(patch) => void model.updateSettings(patch)}
      onClose={() => (settingsSection = null)}
    />
  {/if}
{:else if model.status.kind === 'failed'}
  <StartupError title={model.status.title} message={model.status.message} />
{:else}
  <div class="loading" aria-busy="true" aria-label={t('startup.loading')}></div>
{/if}

<Notifications notices={model.notices} onDismiss={(id) => model.dismiss(id)} />

<style>
  .shell {
    display: flex;
    height: 100%;
  }

  .note {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    background: var(--bg-app);
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .column {
    max-width: var(--note-column-width);
    margin: 0 auto;
    padding: 28px 32px 0;
  }

  .loading {
    height: 100%;
  }
</style>
