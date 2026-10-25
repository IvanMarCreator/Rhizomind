<!--
  Область заметки (С3.3): заголовок — имя файла, подсказка «только чтение»,
  текст в CodeMirror. Сохранение — sessions.ts: через 1 с паузы ввода, при
  переходе к другой заметке (смена `path`) и при закрытии окна (pagehide,
  beforeunload, скрытие окна). Состояние для индикатора — `onStatus`.
-->
<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { VaultPath } from '../api/index.ts';
  import { userMessage } from '../i18n/errors.ts';
  import { t } from '../i18n/index.ts';
  import NoteText from './NoteText.svelte';
  import ReadOnlyNotice from './ReadOnlyNotice.svelte';
  import { coreNoteStore, type NoteStore, type SaveStatus } from './session.ts';
  import { EditorSessions, noteName, type EditorPane } from './sessions.ts';

  let {
    path,
    notify,
    onStatus,
    store = coreNoteStore,
    onOpenSettings,
    showTitle = true,
    focusOnOpen = false,
  }: {
    path: VaultPath | null;
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

  onMount(() => {
    // Закрытие окна: запросы записи уходят синхронно, до выгрузки страницы.
    const flush = () => void sessions.flushAll();
    const onVisibility = () => {
      if (document.visibilityState === 'hidden') flush();
    };
    window.addEventListener('pagehide', flush);
    window.addEventListener('beforeunload', flush);
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      window.removeEventListener('pagehide', flush);
      window.removeEventListener('beforeunload', flush);
      document.removeEventListener('visibilitychange', onVisibility);
      sessions.close();
    };
  });
</script>

{#if pane.kind !== 'empty'}
  <article class="column">
    {#if pane.kind === 'open'}
      {@const session = pane.session}
      {#if showTitle}<h1 class="title">{noteName(session.path)}</h1>{/if}
      {#if session.readOnly}
        <ReadOnlyNotice reason={session.readOnly} size={session.size} {onOpenSettings} />
      {/if}
      {#key session}
        <NoteText {session} focus={focusOnOpen} locked={saveStatus?.kind === 'deleted'} />
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
