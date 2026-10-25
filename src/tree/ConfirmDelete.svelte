<!--
  Подтверждение удаления в корзину (С3.2; включается настройкой хранилища
  «подтверждение удаления»). Безвозвратное удаление на диске без корзины
  подтверждается отдельно окном ядра (инвариант 4), не здесь.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import type { TreeEntry } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import { displayName } from './model.ts';

  let { entry, onConfirm, onCancel }: { entry: TreeEntry; onConfirm: () => void; onCancel: () => void } = $props();

  const titleId = `confirm-delete-${Math.random().toString(36).slice(2)}`;
  const isFolder = $derived(entry.kind === 'folder');
  const name = $derived(displayName(entry));
  let confirmButton: HTMLButtonElement | undefined = $state();
  let dialog: HTMLDivElement | undefined = $state();

  onMount(() => confirmButton?.focus());

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      onCancel();
    } else if (event.key === 'Tab') {
      // Окно модальное: фокус ходит по его кнопкам по кругу.
      event.preventDefault();
      const buttons = dialog ? [...dialog.querySelectorAll('button')] : [];
      const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const step = event.shiftKey ? -1 : 1;
      buttons[(index + step + buttons.length) % buttons.length]?.focus();
    }
  }
</script>

<div class="scrim">
  <div bind:this={dialog} class="dialog" role="dialog" aria-modal="true" aria-labelledby={titleId} tabindex="-1" {onkeydown}>
    <div class="content">
      <h2 class="title" id={titleId}>
        <span class="badge"><Icon name="triangle-alert" size={16} /></span>
        {isFolder ? t('tree.delete.titleFolder', { name }) : t('tree.delete.titleFile', { name })}
      </h2>
      <p class="body">{isFolder ? t('tree.delete.bodyFolder') : t('tree.delete.bodyFile')}</p>
    </div>
    <div class="actions">
      <button type="button" class="button" onclick={onCancel}>{t('tree.delete.cancel')}</button>
      <button type="button" class="button primary" bind:this={confirmButton} onclick={onConfirm}>
        {t('tree.delete.confirm')}
      </button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 30;
    display: grid;
    place-items: center;
    background: var(--bg-scrim);
  }

  .dialog {
    width: 460px;
    max-width: calc(100vw - 32px);
    overflow: hidden;
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    background: var(--bg-popover);
    box-shadow: 0 16px 48px rgb(0 0 0 / 22%);
  }

  .dialog:focus {
    outline: none;
  }

  .content {
    padding: 20px;
  }

  .title {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0;
    color: var(--text-primary);
    font-size: 15px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .badge {
    display: grid;
    place-items: center;
    flex: none;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: var(--status-warning-subtle);
    color: var(--status-warning);
  }

  .body {
    margin: 12px 0 0 40px;
    color: var(--text-secondary);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 20px;
    border-top: 1px solid var(--border-default);
    background: var(--bg-sidebar);
  }

  .button {
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg-app);
    color: var(--text-primary);
    font-weight: 500;
  }

  .button:hover {
    background: var(--bg-hover);
  }

  .button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--text-on-accent);
  }

  .button.primary:hover {
    filter: brightness(1.08);
  }

  .button:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }
</style>
