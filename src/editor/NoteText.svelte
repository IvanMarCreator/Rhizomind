<!--
  Текст заметки в CodeMirror 6. Каждая правка уходит в сессию сохранения
  (session.ts); документ не переписывается при открытии (codemirror.ts).
-->
<script lang="ts">
  import { EditorView } from '@codemirror/view';
  import { onMount } from 'svelte';
  import { createEditorState } from './codemirror.ts';
  import type { NoteSession } from './session.ts';

  let { session, focus = false }: { session: NoteSession; focus?: boolean } = $props();
  let host: HTMLDivElement;

  onMount(() => {
    const view = new EditorView({
      parent: host,
      state: createEditorState(session.text, {
        readOnly: session.readOnly !== null,
        onChange: (text) => session.edit(text),
      }),
    });
    if (focus && !session.readOnly) view.focus();
    return () => view.destroy();
  });
</script>

<div class="text" bind:this={host}></div>

<style>
  .text :global(.cm-editor) {
    color: var(--text-primary);
    font-family: var(--font-ui);
    font-size: 16px;
    line-height: 26px;
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
</style>
