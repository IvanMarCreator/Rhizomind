<!--
  Переключатель хранилищ внизу боковой панели (С3.1, Э10б): имя текущего
  хранилища; меню — известные хранилища (другое открывается в новом окне,
  открытое получает фокус: vault_open) и «Управление хранилищами…»
  (vault_manager_open — отдельное окно, src/vaults/VaultManager.svelte).
-->
<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { core } from '../api/index.ts';
  import type { CurrentVault, VaultListItem } from '../api/index.ts';
  import { userMessage } from '../i18n/errors.ts';
  import { t } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import { vaultLetter } from './letter.ts';
  import { closeOnOutsidePointer, focusMenuItem, onMenuKeydown } from './menu.ts';

  let { vault, onNotice }: { vault: CurrentVault | null; onNotice?: ((text: string) => void) | undefined } = $props();

  const label = $derived(vault ? t('vaults.switcher.current', { name: vault.name }) : t('vaults.switcher.none'));

  let open = $state(false);
  let vaults = $state<VaultListItem[]>([]);
  let button: HTMLButtonElement | undefined = $state();
  let menu: HTMLDivElement | undefined = $state();

  onMount(() =>
    closeOnOutsidePointer(
      () => menu,
      () => button,
      () => (open = false),
    ),
  );

  function notify(error: unknown) {
    onNotice?.(userMessage(error));
  }

  async function toggle() {
    if (open) {
      open = false;
      return;
    }
    vaults = [];
    open = true;
    try {
      vaults = (await core.vaultList()).vaults;
    } catch (error) {
      notify(error);
    }
    // Фокус — на текущее хранилище (или первый пункт) после отрисовки списка.
    await tick();
    if (!open) return;
    const index = vaults.findIndex((item) => item.id === vault?.id);
    focusMenuItem(menu, Math.max(index, 0));
  }

  function close() {
    open = false;
    button?.focus();
  }

  async function choose(item: VaultListItem) {
    open = false;
    if (item.id === vault?.id) return;
    try {
      await core.vaultOpen({ id: item.id });
    } catch (error) {
      notify(error);
    }
  }

  async function manage() {
    open = false;
    try {
      await core.vaultManagerOpen();
    } catch (error) {
      notify(error);
    }
  }
</script>

<div class="switcher-wrap">
  {#if open}
    <div
      bind:this={menu}
      class="menu"
      role="menu"
      tabindex="-1"
      aria-label={t('vaults.menu.label')}
      onkeydown={(event) => onMenuKeydown(event, menu, close)}
    >
      <p class="menu-heading" aria-hidden="true">{t('vaults.menu.label')}</p>
      {#each vaults as item (item.id)}
        {@const current = item.id === vault?.id}
        <button
          type="button"
          class="item"
          role="menuitemradio"
          aria-checked={current}
          onclick={() => void choose(item)}
        >
          <span class="item-letter" class:current aria-hidden="true">{vaultLetter(item.name)}</span>
          <span class="item-name">{item.name}</span>
          {#if current}
            <span class="check"><Icon name="check" size={14} /></span>
          {/if}
        </button>
      {/each}
      <div class="separator" role="separator"></div>
      <button type="button" class="item" role="menuitem" onclick={() => void manage()}>
        <span class="item-icon"><Icon name="library" size={15} /></span>
        <span class="item-name">{t('vaults.menu.manage')}</span>
      </button>
    </div>
  {/if}
  <button
    bind:this={button}
    type="button"
    class="switcher"
    aria-label={label}
    title={t('vaults.switcher.open')}
    aria-haspopup="menu"
    aria-expanded={open}
    onclick={() => void toggle()}
  >
    {#if vault}
      <span class="badge" aria-hidden="true">{vaultLetter(vault.name)}</span>
      <span class="name">{vault.name}</span>
    {:else}
      <span class="name none">{t('vaults.switcher.none')}</span>
    {/if}
    <span class="chevrons"><Icon name="chevrons-up-down" size={14} /></span>
  </button>
</div>

<style>
  .switcher-wrap {
    position: relative;
    width: 100%;
  }

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

  .switcher:hover,
  .switcher[aria-expanded='true'] {
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

  /* Меню хранилищ (tokens.md): ширина 240, отступ 4, пункты 30, скругление 8 / 5;
     над переключателем, с зазором 2. */
  .menu {
    position: absolute;
    bottom: calc(100% + 2px);
    left: 2px;
    z-index: 20;
    width: 240px;
    max-height: 60vh;
    overflow: auto;
    padding: 4px;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    background: var(--bg-popover);
    box-shadow: 0 8px 24px rgb(0 0 0 / 14%);
    box-sizing: border-box;
  }

  .menu:focus {
    outline: none;
  }

  .menu-heading {
    margin: 0;
    padding: 6px 8px 4px;
    color: var(--text-muted);
    font-size: 11px;
    font-weight: 600;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 30px;
    padding: 0 8px;
    border-radius: 5px;
    text-align: left;
  }

  .item:hover,
  .item:focus-visible {
    outline: none;
    background: var(--bg-hover);
  }

  .item-letter {
    display: grid;
    place-items: center;
    flex: none;
    width: 18px;
    height: 18px;
    border-radius: 4px;
    background: var(--bg-active);
    color: var(--text-secondary);
    font-size: 10px;
    font-weight: 600;
  }

  .item-letter.current {
    background: var(--accent-subtle);
    color: var(--accent-text);
  }

  .item-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--text-primary);
    font-size: 13px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .item-icon {
    display: grid;
    place-items: center;
    width: 18px;
    color: var(--icon-default);
  }

  .check {
    color: var(--accent);
  }

  .separator {
    height: 1px;
    margin: 4px 0;
    background: var(--border-default);
  }
</style>
