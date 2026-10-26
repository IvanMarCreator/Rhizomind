<!--
  Заголовок заметки над текстом = имя файла (С3.3); клик открывает поле
  переименования с той же проверкой имени, что в дереве (С3.2, Э4б).
  Встраивается над редактором (задача 2.4); операция — через дерево, чтобы
  новое имя сразу было видно и в дереве, и в пути над заметкой.
-->
<script lang="ts">
  import { tick } from 'svelte';
  import type { VaultPath } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import NameField from './NameField.svelte';
  import { isUndo } from './keys.ts';
  import { parentOf, samePath, splitName } from './model.ts';
  import type { FileTreeState } from './tree.svelte.ts';

  let { tree, path }: { tree: FileTreeState; path: VaultPath } = $props();

  const parts = $derived(splitName({ kind: 'note', path }));
  /**
   * Заметка, на которой открыто поле. Путь фиксируется при открытии: сохранение
   * переименовывает именно её, даже если тем временем открыли другую заметку;
   * смена заметки закрывает поле.
   */
  let editingPath = $state.raw<VaultPath | null>(null);
  const editing = $derived(editingPath !== null && samePath(editingPath, path));
  let button: HTMLButtonElement | undefined = $state();

  function startEditing() {
    editingPath = [...path];
  }

  async function finishEditing() {
    editingPath = null;
    await tick();
    button?.focus();
  }
</script>

<div class="note-title">
  {#if editing && editingPath}
    {@const target = editingPath}
    <NameField
      variant="title"
      kind="note"
      dir={parentOf(target)}
      stem={parts.stem}
      ext={parts.ext}
      renaming={target}
      submit={(name) => tree.rename(target, name)}
      onDone={() => void finishEditing()}
    />
  {:else}
    <h1 class="heading">
      <button
        bind:this={button}
        type="button"
        class="name"
        title={t('note.title.renameHint')}
        aria-label={parts.stem}
        onclick={startEditing}
        onkeydown={(event) => {
          if (!isUndo(event)) return;
          event.preventDefault();
          void tree.undoRename();
        }}
      >
        {parts.stem}
      </button>
    </h1>
  {/if}
</div>

<style>
  .note-title {
    margin: 0 0 10px;
  }

  .heading {
    margin: 0;
    font-size: 28px;
    font-weight: 700;
    line-height: 36px;
  }

  .name {
    max-width: 100%;
    padding: 0 4px;
    margin-left: -4px;
    border-radius: 4px;
    color: var(--text-primary);
    font: inherit;
    text-align: left;
    overflow-wrap: anywhere;
    user-select: text;
  }

  .name:hover {
    background: var(--bg-hover);
  }
</style>
