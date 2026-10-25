<!--
  Переключатель хранилищ внизу боковой панели (Э10б) — заготовка: показывает
  хранилище окна. Список хранилищ и «Управление хранилищами…» — задача 2.8.
-->
<script lang="ts">
  import type { CurrentVault } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import Icon from './Icon.svelte';

  let { vault, onOpen }: { vault: CurrentVault | null; onOpen?: () => void } = $props();

  const label = $derived(vault ? t('vaults.switcher.current', { name: vault.name }) : t('vaults.switcher.none'));
  const letter = $derived(vault ? Array.from(vault.name.trim())[0]?.toLocaleUpperCase('ru') ?? '' : '');
</script>

<button type="button" class="switcher" aria-label={label} title={t('vaults.switcher.open')} onclick={() => onOpen?.()}>
  {#if vault}
    <span class="badge" aria-hidden="true">{letter}</span>
    <span class="name">{vault.name}</span>
  {:else}
    <span class="name none">{t('vaults.switcher.none')}</span>
  {/if}
  <span class="chevrons"><Icon name="chevrons-up-down" size={14} /></span>
</button>

<style>
  .switcher {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 32px;
    padding: 0 8px 0 6px;
    border-radius: 5px;
    text-align: left;
  }

  .switcher:hover {
    background: var(--bg-hover);
  }

  .badge {
    display: grid;
    place-items: center;
    flex: none;
    width: 20px;
    height: 20px;
    border-radius: 5px;
    background: var(--accent-subtle);
    color: var(--accent-text);
    font-size: 11px;
    font-weight: 600;
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--text-primary);
    font-weight: 500;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .name.none {
    color: var(--text-muted);
    font-weight: 400;
  }

  .chevrons {
    color: var(--icon-muted);
  }
</style>
