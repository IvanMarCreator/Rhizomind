<!-- Боковая панель: заголовок «Файлы», дерево файлов (задача 2.3), внизу переключатель хранилищ (2.8а). -->
<script lang="ts">
  import type { CurrentVault } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import FileTree from '../tree/FileTree.svelte';
  import TreeActions from '../tree/TreeActions.svelte';
  import type { FileTreeState } from '../tree/tree.svelte.ts';
  import VaultSwitcher from '../vaults/VaultSwitcher.svelte';

  let {
    vault,
    files,
    onNotice,
  }: { vault: CurrentVault | null; files: FileTreeState; onNotice?: (text: string) => void } = $props();

  // Кнопки создания в корне видны при наведении на панель и при фокусе внутри неё (решение 48).
  let hovered = $state(false);
  let focused = $state(false);
</script>

<aside
  class="sidebar"
  aria-label={t('shell.sidebar.label')}
  data-revealed={vault !== null && (hovered || focused) ? 'true' : 'false'}
  onmouseenter={() => (hovered = true)}
  onmouseleave={() => (hovered = false)}
  onfocusin={() => (focused = true)}
  onfocusout={(event) => (focused = event.currentTarget.contains(event.relatedTarget as Node | null))}
>
  <div class="header">
    <h2 class="heading">{t('shell.sidebar.files')}</h2>
    {#if vault}
      <div class="header-actions">
        <TreeActions tree={files} />
      </div>
    {/if}
  </div>
  <div class="tree">
    {#if vault}
      <FileTree tree={files} />
    {/if}
  </div>
  <div class="footer">
    <VaultSwitcher {vault} {onNotice} />
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

  .header {
    display: flex;
    align-items: center;
    height: var(--sidebar-header-height);
    padding: 0 8px 0 14px;
  }

  .header-actions {
    margin-left: auto;
    opacity: 0;
    transition: opacity 0.1s ease;
  }

  .sidebar[data-revealed='true'] .header-actions {
    opacity: 1;
  }

  .heading {
    display: flex;
    align-items: center;
    margin: 0;
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
