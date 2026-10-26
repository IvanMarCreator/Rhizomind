<!--
  Выбор папки для новых заметок внутри хранилища (Э12.3, «Выбрать…»):
  список папок дерева (без скрытых) с поиском. Esc — отмена.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { core, type VaultPath } from '../api/index.ts';
  import { userMessage } from '../i18n/errors.ts';
  import { t } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import { isHidden } from '../tree/model.ts';

  let {
    onPick,
    onCancel,
  }: { onPick: (path: VaultPath) => void; onCancel: () => void } = $props();

  const collator = new Intl.Collator('ru', { numeric: true, sensitivity: 'base' });
  let folders = $state.raw<VaultPath[]>([]);
  let error = $state<string | null>(null);
  let filter = $state('');
  let input: HTMLInputElement | undefined = $state();

  const shown = $derived.by(() => {
    const needle = filter.trim().toLocaleLowerCase('ru');
    const all: VaultPath[] = [[], ...folders];
    return needle ? all.filter((path) => path.join('/').toLocaleLowerCase('ru').includes(needle)) : all;
  });

  onMount(() => {
    input?.focus();
    core
      .treeList()
      .then(({ entries }) => {
        folders = entries
          .filter((entry) => entry.kind === 'folder' && !isHidden(entry.path))
          .map((entry) => entry.path)
          .sort((a, b) => collator.compare(a.join('\u0000'), b.join('\u0000')));
      })
      .catch((failure: unknown) => {
        error = userMessage(failure);
      });
  });

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      onCancel();
    }
  }
</script>

<div class="picker" role="dialog" aria-modal="true" aria-label={t('settings.folderPicker.title')} tabindex="-1" {onkeydown}>
  <div class="search">
    <Icon name="search" size={14} />
    <input bind:this={input} bind:value={filter} placeholder={t('settings.folderPicker.filter')} aria-label={t('settings.folderPicker.filter')} />
  </div>
  <ul class="list" role="listbox" aria-label={t('settings.folderPicker.title')}>
    {#each shown as path (path.join('/'))}
      <li role="option" aria-selected="false">
        <button type="button" class="folder" onclick={() => onPick(path)} style:padding-left={`${10 + Math.max(0, path.length - 1) * 14}px`}>
          <Icon name="folder" size={14} />
          <span>{path.length === 0 ? t('settings.newNote.rootFolder') : path.at(-1)}</span>
        </button>
      </li>
    {:else}
      <li class="empty">{error ?? t('settings.folderPicker.empty')}</li>
    {/each}
  </ul>
  <div class="actions">
    <button type="button" class="button" onclick={onCancel}>{t('settings.folderPicker.cancel')}</button>
  </div>
</div>

<style>
  .picker {
    position: absolute;
    top: 100%;
    right: 0;
    z-index: 2;
    width: 300px;
    margin-top: 4px;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    background: var(--bg-popover);
    box-shadow: 0 10px 30px rgb(0 0 0 / 18%);
  }

  .picker:focus {
    outline: none;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border-default);
    color: var(--icon-muted);
  }

  .search input {
    flex: 1;
    min-width: 0;
    border: none;
    background: none;
    color: var(--text-primary);
    font: inherit;
    outline: none;
  }

  .list {
    max-height: 220px;
    margin: 0;
    padding: 4px;
    overflow: auto;
    list-style: none;
  }

  .folder {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 5px 10px;
    border-radius: 4px;
    color: var(--text-primary);
    text-align: left;
  }

  .folder span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder :global(.icon) {
    color: var(--icon-default);
  }

  .folder:hover,
  .folder:focus-visible {
    background: var(--bg-hover);
    outline: none;
  }

  .empty {
    padding: 8px 10px;
    color: var(--text-muted);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    padding: 6px 8px;
    border-top: 1px solid var(--border-default);
  }

  .button {
    padding: 4px 10px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    color: var(--text-primary);
  }
</style>
