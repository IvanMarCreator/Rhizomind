<!--
  Текст заметки в CodeMirror 6. Каждая правка уходит в сессию сохранения
  (session.ts); документ не переписывается при открытии (codemirror.ts).
-->
<script lang="ts">
  import { EditorView } from '@codemirror/view';
  import './codemirror-base.css';
  import { onMount, untrack } from 'svelte';
  import type { EditorMode } from '../api/index.ts';
  import { createEditorState, replaceDocument, setMode, setReadOnly } from './codemirror.ts';
  import type { NoteSession } from './session.ts';
  import type { WikiHost } from './wikilinks/host.ts';

  let {
    session,
    mode,
    onOpenLink,
    wiki,
    focus = false,
    locked = false,
  }: {
    session: NoteSession;
    mode: EditorMode;
    /** Щелчок по внешней ссылке в живом предпросмотре. */
    onOpenLink: (url: string) => void;
    /** Вики-ссылки: переход, разрешение, подсказки (src/editor/wikilinks). */
    wiki?: WikiHost;
    focus?: boolean;
    /** Заметку удалили: правка запрещена (ввод уже отброшен). */
    locked?: boolean;
  } = $props();
  let host: HTMLDivElement;
  let view: EditorView | undefined = $state.raw();

  $effect(() => {
    if (view) setReadOnly(view, locked || session.readOnly !== null);
  });

  // Режим меняет только оформление: документ и выделение те же.
  let shownMode: EditorMode = untrack(() => mode);
  $effect(() => {
    if (!view || mode === shownMode) return;
    shownMode = mode;
    setMode(view, mode, (url) => onOpenLink(url));
  });

  onMount(() => {
    const created = new EditorView({
      parent: host,
      state: createEditorState(session.text, {
        readOnly: session.readOnly !== null,
        mode: shownMode,
        onOpenLink: (url) => onOpenLink(url),
        ...(wiki ? { wiki } : {}),
        onChange: (text) => session.edit(text),
      }),
    });
    if (focus && !session.readOnly) created.focus();
    view = created;
    // Заметку изменили снаружи, ввода не было: текст с диска, курсор на месте (задача 2.9).
    const stop = session.onExternalText((text) => replaceDocument(created, text));
    return () => {
      stop();
      created.destroy();
    };
  });
</script>

<div class="text" bind:this={host}></div>

<style>
  .text :global(.cm-editor) {
    color: var(--text-primary);
    font-family: var(--font-ui);
    /* Размер шрифта текста — настройка хранилища (src/settings/apply.ts). */
    font-size: var(--note-font-size, 16px);
    line-height: 1.625;
  }

  .text :global(.cm-editor.cm-focused) {
    outline: none;
  }

  .text :global(.cm-scroller) {
    font-family: inherit;
    line-height: inherit;
    overflow: visible;
  }

  .text :global(.cm-content) {
    padding: 0 0 40vh;
    caret-color: var(--accent);
  }

  .text :global(.cm-line) {
    padding: 0;
  }

  .text :global(.cm-cursor) {
    border-left: 1.5px solid var(--accent);
  }

  .text :global(.cm-specialChar) {
    color: var(--status-error);
  }

  /* Живой предпросмотр — src/editor/live-preview/live-preview.css. */
</style>
