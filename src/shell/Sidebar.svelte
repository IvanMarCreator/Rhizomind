<!-- Боковая панель: заголовок «Файлы», дерево файлов (задача 2.3), внизу переключатель хранилищ. -->
<script lang="ts">
  import type { CurrentVault } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import FileTree from '../tree/FileTree.svelte';
  import type { FileTreeState } from '../tree/tree.svelte.ts';
  import VaultSwitcher from './VaultSwitcher.svelte';

  let { vault, files }: { vault: CurrentVault | null; files: FileTreeState } = $props();
</script>

<aside class="sidebar" aria-label={t('shell.sidebar.label')}>
  <h2 class="heading">{t('shell.sidebar.files')}</h2>
  <div class="tree">
    {#if vault}
      <FileTree tree={files} />
    {/if}
  </div>
  <div class="footer">
    <VaultSwitcher {vault} />
  </div>
</aside>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    width: var(--sidebar-width);
    min-width: 0;
    background: var(--bg-sidebar);
    border-right: 1px solid var(--border-default);
    box-sizing: border-box;
  }

  .heading {
    display: flex;
    align-items: center;
    height: var(--sidebar-header-height);
    margin: 0;
    padding: 0 14px;
    color: var(--text-muted);
    font-size: var(--font-size-heading);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .tree {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .footer {
    display: flex;
    align-items: center;
    height: var(--sidebar-footer-height);
    padding: 0 8px;
  }
</style>
