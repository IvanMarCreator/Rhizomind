<!--
  Кнопки «Новая заметка» и «Новая папка» для корня хранилища (решение
  владельца 48, как в Obsidian): в заголовке панели «ФАЙЛЫ». Когда они видны
  (наведение на панель, фокус), решает боковая панель.
-->
<script lang="ts">
  import { t } from '../i18n/index.ts';
  import TreeIcon from './TreeIcon.svelte';
  import type { FileTreeState } from './tree.svelte.ts';

  let { tree }: { tree: FileTreeState } = $props();
</script>

<div class="actions" role="group" aria-label={t('tree.rootActions')}>
  <button
    type="button"
    class="action"
    aria-label={t('tree.newNote')}
    title={t('tree.newNote')}
    onclick={() => void tree.createNote([])}
  >
    <TreeIcon name="square-pen" />
  </button>
  <button
    type="button"
    class="action"
    aria-label={t('tree.newFolder')}
    title={t('tree.newFolder')}
    onclick={() => void tree.createFolder([])}
  >
    <TreeIcon name="folder-plus" />
  </button>
</div>

<style>
  .actions {
    display: flex;
    gap: 2px;
  }

  .action {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 4px;
    color: var(--icon-default);
  }

  .action:hover {
    background: var(--bg-hover);
  }
</style>
