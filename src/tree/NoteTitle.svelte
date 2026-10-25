<!--
  Заголовок заметки над текстом = имя файла (С3.3); клик открывает поле
  переименования с той же проверкой имени, что в дереве (С3.2, Э4б).
  Встраивается над редактором (задача 2.4); операция — через дерево, чтобы
  новое имя сразу было видно и в дереве, и в пути над заметкой.
-->
<script lang="ts">
  import type { VaultPath } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import NameField from './NameField.svelte';
  import { parentOf, splitName } from './model.ts';
  import type { FileTreeState } from './tree.svelte.ts';

  let { tree, path }: { tree: FileTreeState; path: VaultPath } = $props();

  const parts = $derived(splitName({ kind: 'note', path }));
  let editing = $state(false);
</script>

<div class="note-title">
  {#if editing}
    <NameField
      variant="title"
      dir={parentOf(path)}
      stem={parts.stem}
      ext={parts.ext}
      renaming={path}
      submit={(name) => tree.rename(path, name)}
      onDone={() => (editing = false)}
    />
  {:else}
    <h1 class="heading">
      <button
        type="button"
        class="name"
        title={t('note.title.renameHint')}
        aria-label={parts.stem}
        onclick={() => (editing = true)}
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
