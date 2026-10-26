<!--
  Текст заметки в CodeMirror 6. Каждая правка уходит в сессию сохранения
  (session.ts); документ не переписывается при открытии (codemirror.ts).
-->
<script lang="ts">
  import { EditorView } from '@codemirror/view';
  import { onMount, untrack } from 'svelte';
  import type { EditorMode } from '../api/index.ts';
  import { createEditorState, setMode, setReadOnly } from './codemirror.ts';
  import type { NoteSession } from './session.ts';

  let {
    session,
    mode,
    onOpenLink,
    focus = false,
    locked = false,
  }: {
    session: NoteSession;
    mode: EditorMode;
    /** Щелчок по внешней ссылке в живом предпросмотре. */
    onOpenLink: (url: string) => void;
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
        onChange: (text) => session.edit(text),
      }),
    });
    if (focus && !session.readOnly) created.focus();
    view = created;
    return () => created.destroy();
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

  /* ---- Живой предпросмотр (src/editor/live-preview, tokens.md) ---- */

  /* Разметка на строке с курсором. */
  .text :global(.cm-rz-markup) {
    color: var(--text-faint);
  }

  .text :global(.cm-line.cm-rz-heading) {
    font-weight: 600;
  }

  .text :global(.cm-line.cm-rz-h1) {
    font-size: 28px;
    font-weight: 700;
    line-height: 36px;
  }

  .text :global(.cm-line.cm-rz-h2) {
    padding-top: 12px;
    font-size: 21px;
    line-height: 27px;
  }

  .text :global(.cm-line.cm-rz-h3) {
    font-size: 18px;
    line-height: 26px;
  }

  .text :global(.cm-line.cm-rz-h4) {
    font-size: 17px;
  }

  .text :global(.cm-rz-strong) {
    font-weight: 600;
  }

  .text :global(.cm-rz-em) {
    font-style: italic;
  }

  .text :global(.cm-rz-strike) {
    text-decoration: line-through;
  }

  .text :global(.cm-rz-code) {
    padding: 1px 3px;
    border-radius: 4px;
    background: var(--bg-code);
    color: var(--text-secondary);
    font-family: var(--font-mono);
    font-size: 0.88em;
  }

  .text :global(.cm-line.cm-rz-codeblock) {
    padding: 0 16px;
    background: var(--bg-code);
    font-family: var(--font-mono);
    font-size: 13px;
    line-height: 20px;
  }

  .text :global(.cm-line.cm-rz-codeblock-first) {
    padding-top: 12px;
    border-radius: 6px 6px 0 0;
  }

  .text :global(.cm-line.cm-rz-codeblock-last) {
    padding-bottom: 12px;
    border-radius: 0 0 6px 6px;
  }

  .text :global(.cm-line.cm-rz-codeblock-first.cm-rz-codeblock-last) {
    border-radius: 6px;
  }

  .text :global(.cm-line.cm-rz-quote) {
    padding-left: 17px;
    border-left: 3px solid var(--border-strong);
    color: var(--text-secondary);
  }

  .text :global(.cm-rz-list-number) {
    color: var(--text-secondary);
  }

  /* Маркер списка (tokens.md «Списки»): колонка 20 px, точка-круг 5 × 5. */
  .text :global(.cm-rz-bullet) {
    display: inline-block;
    width: 20px;
    text-align: center;
    vertical-align: middle;
    line-height: 0;
  }

  .text :global(.cm-rz-bullet)::before {
    content: '';
    display: inline-block;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--text-secondary);
  }

  .text :global(.cm-rz-checkbox) {
    display: inline-block;
    width: 16px;
    height: 16px;
    margin-right: 2px;
    border: 1.5px solid var(--border-strong);
    border-radius: 4px;
    vertical-align: -2px;
    box-sizing: border-box;
    cursor: pointer;
  }

  .text :global(.cm-rz-checkbox) {
    position: relative;
  }

  .text :global(.cm-rz-checkbox-checked) {
    border-color: var(--accent);
    background: var(--accent);
  }

  /* Галочка рисуется рамкой: без картинок и data:-адресов. */
  .text :global(.cm-rz-checkbox-checked)::after {
    content: '';
    position: absolute;
    left: 4px;
    top: 1px;
    width: 4px;
    height: 8px;
    border: solid var(--text-on-accent);
    border-width: 0 2px 2px 0;
    transform: rotate(45deg);
  }

  .text :global(.cm-rz-checkbox[aria-disabled='true']) {
    cursor: default;
  }

  .text :global(.cm-line.cm-rz-task-done) {
    color: var(--text-muted);
    text-decoration: line-through;
  }

  .text :global(.cm-rz-link) {
    color: var(--accent-text);
    text-decoration: underline;
    text-underline-offset: 3px;
    cursor: pointer;
  }

  .text :global(.cm-rz-rule) {
    display: inline-block;
    width: 100%;
    height: 1px;
    background: var(--border-strong);
    vertical-align: middle;
  }
</style>
