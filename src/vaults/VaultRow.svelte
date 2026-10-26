<!--
  Строка списка в окне «Управление хранилищами» (Э11а): буква, имя, путь,
  метка «открыто». При наведении — «Открыть» и меню «…» с пунктом «Убрать из
  списка» (файлы не удаляются). Щелчок по строке тоже открывает хранилище.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import type { VaultListItem } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import { vaultLetter } from './letter.ts';
  import { closeOnOutsidePointer, focusMenuItem, onMenuKeydown } from './menu.ts';

  let {
    vault,
    disabled = false,
    onOpen,
    onRemove,
  }: { vault: VaultListItem; disabled?: boolean; onOpen: () => void; onRemove: () => void } = $props();

  let menuOpen = $state(false);
  let menu: HTMLDivElement | undefined = $state();
  let moreButton: HTMLButtonElement | undefined = $state();

  onMount(() =>
    closeOnOutsidePointer(
      () => menu,
      () => moreButton,
      () => (menuOpen = false),
    ),
  );

  $effect(() => {
    if (menuOpen && menu) focusMenuItem(menu, 0);
  });

  function closeMenu() {
    menuOpen = false;
    moreButton?.focus();
  }

  function remove() {
    menuOpen = false;
    onRemove();
  }
</script>

<li class="row" class:menu-open={menuOpen}>
  <button type="button" class="main" {disabled} onclick={onOpen}>
    <span class="letter" class:is-open={vault.is_open} aria-hidden="true">{vaultLetter(vault.name)}</span>
    <span class="text">
      <span class="line">
        <span class="name">{vault.name}</span>
        {#if vault.is_open}
          <span class="pill">{t('vaults.manager.isOpen')}</span>
        {/if}
      </span>
      <span class="path">{vault.path}</span>
    </span>
  </button>
  <span class="tools">
    <button
      type="button"
      class="open-button"
      {disabled}
      aria-label={t('vaults.manager.openNamed', { name: vault.name })}
      onclick={onOpen}
    >
      {t('vaults.manager.open')}
    </button>
    <button
      type="button"
      class="more"
      bind:this={moreButton}
      aria-label={t('vaults.manager.more', { name: vault.name })}
      title={t('vaults.manager.more', { name: vault.name })}
      aria-haspopup="menu"
      aria-expanded={menuOpen}
      onclick={() => (menuOpen = !menuOpen)}
    >
      <Icon name="ellipsis" size={16} />
    </button>
  </span>
  {#if menuOpen}
    <div
      bind:this={menu}
      class="menu"
      role="menu"
      tabindex="-1"
      aria-label={t('vaults.manager.more', { name: vault.name })}
      onkeydown={(event) => onMenuKeydown(event, menu, closeMenu)}
    >
      <button type="button" class="item" role="menuitem" onclick={remove}>
        <span class="item-icon"><Icon name="circle-minus" size={15} /></span>
        <span class="item-text">
          <span class="item-label">{t('vaults.manager.remove')}</span>
          <span class="item-hint">{t('vaults.manager.removeHint')}</span>
        </span>
      </button>
    </div>
  {/if}
</li>

<style>
  .row {
    position: relative;
    display: flex;
    align-items: center;
    height: 56px;
    border-radius: 6px;
  }

  .row:hover,
  .row:focus-within,
  .row.menu-open {
    background: var(--bg-hover);
  }

  .main {
    display: flex;
    align-items: center;
    gap: 12px;
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0 8px 0 10px;
    border-radius: 6px;
    text-align: left;
  }

  .main:focus-visible {
    outline-offset: -2px;
  }

  .letter {
    display: grid;
    place-items: center;
    flex: none;
    width: 32px;
    height: 32px;
    border-radius: 7px;
    background: var(--bg-active);
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 600;
  }

  .letter.is-open {
    background: var(--accent-subtle);
    color: var(--accent-text);
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .name,
  .path {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .name {
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 500;
  }

  .pill {
    display: flex;
    align-items: center;
    flex: none;
    height: 18px;
    padding: 0 6px;
    border-radius: 9px;
    background: var(--accent-subtle);
    color: var(--accent-text);
    font-size: 11px;
    font-weight: 500;
  }

  .path {
    color: var(--text-muted);
    font-size: 12px;
  }

  /* «Открыть» и «…» поверх правого края строки при наведении и фокусе:
     в покое имя и путь занимают всю ширину. */
  .tools {
    position: absolute;
    top: 0;
    right: 8px;
    bottom: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding-left: 8px;
    background: var(--bg-hover);
    opacity: 0;
    pointer-events: none;
  }

  .row:hover .tools,
  .row:focus-within .tools,
  .row.menu-open .tools {
    opacity: 1;
    pointer-events: auto;
  }

  .open-button {
    height: 28px;
    padding: 0 10px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg-app);
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 500;
  }

  .open-button:hover {
    background: var(--bg-hover);
  }

  .more {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 5px;
    color: var(--icon-default);
  }

  .more:hover,
  .row.menu-open .more {
    background: var(--bg-active);
  }

  .menu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 20;
    width: 270px;
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

  .item {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    width: 100%;
    padding: 7px 8px;
    border-radius: 5px;
    text-align: left;
  }

  .item:hover,
  .item:focus-visible {
    outline: none;
    background: var(--bg-hover);
  }

  .item-icon {
    padding-top: 1px;
    color: var(--icon-default);
  }

  .item-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .item-label {
    color: var(--text-primary);
    font-size: 13px;
  }

  .item-hint {
    color: var(--text-muted);
    font-size: 12px;
    white-space: nowrap;
  }
</style>
