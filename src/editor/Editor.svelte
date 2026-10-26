<!--
  Область заметки (С3.3): заголовок — имя файла, подсказка «только чтение»,
  текст в CodeMirror. Сохранение — sessions.ts: через 1 с паузы ввода, при
  переходе к другой заметке (смена `path`) и при закрытии окна: по сигналу
  ядра (`flushForClose`, оболочка — src/shell/closing.ts, задача 0.7) и
  запасным путём по pagehide, beforeunload, скрытию окна. Состояние для
  индикатора — `onStatus`.
-->
<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { core, onCoreEvent, type EditorMode, type VaultPath } from '../api/index.ts';
  import { userMessage } from '../i18n/errors.ts';
  import { t } from '../i18n/index.ts';
  import NoteText from './NoteText.svelte';
  import ReadOnlyNotice from './ReadOnlyNotice.svelte';
  import { coreNoteStore, type NoteStore, type SaveStatus } from './session.ts';
  import { EditorSessions, noteName, type EditorPane, type UnsavedNote } from './sessions.ts';
  import { WindowWikiHost } from './wikilinks/window-host.ts';

  let {
    path,
    mode = 'live_preview',
    notify,
    onStatus,
    store = coreNoteStore,
    onOpenSettings,
    showTitle = true,
    focusOnOpen = false,
    onOpenNote,
  }: {
    path: VaultPath | null;
    /** Живой предпросмотр или исходный текст (решение 72). */
    mode?: EditorMode;
    notify: (text: string) => void;
    onStatus: (status: SaveStatus | null) => void;
    store?: NoteStore;
    onOpenSettings?: () => void;
    /**
     * Заголовок-имя над текстом. Выключается, когда оболочка рисует свой
     * заголовок с переименованием (NoteTitle, задача 2.3).
     */
    showTitle?: boolean;
    /**
     * Ставить ли курсор в текст при открытии. По умолчанию нет: фокус может
     * быть у поля имени новой заметки в дереве (2.3) — его нельзя отнимать.
     */
    focusOnOpen?: boolean;
    /** Переход по вики-ссылке: открыть заметку в окне (история переходов). */
    onOpenNote?: (path: VaultPath) => void;
  } = $props();

  let pane = $state.raw<EditorPane>({ kind: 'empty' });
  let saveStatus = $state.raw<SaveStatus | null>(null);

  const sessions = new EditorSessions(
    untrack(() => store),
    {
      onPane: (next) => (pane = next),
      onStatus: (next) => {
        saveStatus = next;
        onStatus(next);
      },
      notify: (text) => notify(text),
    },
  );

  $effect(() => {
    sessions.show(path);
  });

  /** Вики-ссылки открытой заметки (С3.4): только заметки хранилища, не `open_external`. */
  const wiki = new WindowWikiHost({
    from: () => (pane.kind === 'open' ? pane.session.path : null),
    openNote: (next) => onOpenNote?.(next),
    notify: (text) => notify(text),
  });

  /**
   * Внешняя ссылка из заметки: только командой ядра — оно показывает
   * подтверждение и передаёт адрес системному браузеру (С3.4). Отмена — не ошибка.
   */
  function openLink(url: string): void {
    core.openExternal({ url }).catch((error: unknown) => notify(userMessage(error)));
  }

  /**
   * Заметку или папку переименовали (дерево, `note:renamed`). Вызывать до
   * того, как окно откроет новый путь: это не уход с заметки, ввод пишется по
   * новому пути с прежней ожидаемой версией.
   */
  export function renamed(from: VaultPath, to: VaultPath): void {
    sessions.renamed(from, to);
  }

  /** Заметку или папку удалили: несохранённый ввод не пишется (решение владельца). */
  export function removed(removedPath: VaultPath): void {
    sessions.removed(removedPath);
  }

  /**
   * Закрытие окна (задача 0.7): записать ввод всех заметок и дождаться
   * ответов; возвращает заметки, ввод которых записать не удалось.
   */
  export function flushForClose(): Promise<UnsavedNote[]> {
    return sessions.flushForClose();
  }

  onMount(() => {
    // Запасной путь закрытия окна (основной — сигнал ядра, flushForClose):
    // запросы записи уходят синхронно, до выгрузки страницы.
    const flush = () => void sessions.flushAll();
    const onVisibility = () => {
      if (document.visibilityState === 'hidden') flush();
    };
    // Дерево изменилось — разрешение вики-ссылок заново (созданная по ссылке заметка и т.п.).
    let disposed = false;
    let unlisten: (() => void) | null = null;
    onCoreEvent('vault:changed', () => {
      if (!disposed) wiki.refresh();
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => undefined);
    window.addEventListener('pagehide', flush);
    window.addEventListener('beforeunload', flush);
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      window.removeEventListener('pagehide', flush);
      window.removeEventListener('beforeunload', flush);
      document.removeEventListener('visibilitychange', onVisibility);
      disposed = true;
      unlisten?.();
      sessions.close();
    };
  });
</script>

{#if pane.kind !== 'empty'}
  <article class="column" class:untitled={!showTitle}>
    {#if pane.kind === 'open'}
      {@const session = pane.session}
      {#if showTitle}<h1 class="title">{noteName(session.path)}</h1>{/if}
      {#if session.readOnly}
        <ReadOnlyNotice reason={session.readOnly} size={session.size} {onOpenSettings} />
      {/if}
      {#key session}
        <NoteText {session} {mode} onOpenLink={openLink} {wiki} focus={focusOnOpen} locked={saveStatus?.kind === 'deleted'} />
      {/key}
    {:else if pane.kind === 'loading'}
      {#if showTitle}<h1 class="title">{noteName(pane.path)}</h1>{/if}
      <p class="message" aria-busy="true">{t('editor.loading')}</p>
    {:else}
      {#if showTitle}<h1 class="title">{noteName(pane.path)}</h1>{/if}
      <p class="message">{t('editor.openFailed', { message: userMessage(pane.error) })}</p>
    {/if}
  </article>
{/if}

<style>
  .column {
    max-width: var(--note-column-width);
    margin: 0 auto;
    padding: 28px 32px 0;
  }

  /* Заголовок рисует оболочка (NoteTitle) в своей колонке над редактором. */
  .column.untitled {
    padding-top: 0;
  }

  .title {
    margin: 0 0 10px;
    color: var(--text-primary);
    font-size: 28px;
    font-weight: 700;
    line-height: 36px;
    overflow-wrap: anywhere;
  }

  .message {
    margin: 0;
    color: var(--text-muted);
  }
</style>
