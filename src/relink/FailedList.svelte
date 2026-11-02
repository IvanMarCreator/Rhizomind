<!--
  Список «Не удалось обновить ссылки в N заметках» (0.2-5.1, С2-3.5, решение
  115, макет У-3): у каждой заметки — причина и «будет повторено / не
  повторяется»; щелчок открывает заметку; «Повторить» — только если есть
  занятые или недоступные заметки. Enter и фокус — на «Закрыть», Esc закрывает.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import type { VaultPath } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import { failureRows, type RelinkNotice } from './notices.ts';

  let {
    notice,
    limitBytes,
    onOpen,
    onRetry,
    onClose,
  }: {
    notice: RelinkNotice;
    limitBytes: number;
    onOpen: (path: VaultPath) => void;
    onRetry: () => void;
    onClose: () => void;
  } = $props();

  const titleId = `relink-failed-${Math.random().toString(36).slice(2)}`;
  const rows = $derived(failureRows(notice.failed, limitBytes));
  const body = $derived(
    notice.reason === 'duplicate' || notice.reason === null ? t('relink.failed.body') : t('relink.failed.bodyRename'),
  );
  let closeButton: HTMLButtonElement | undefined = $state();
  let dialog: HTMLDivElement | undefined = $state();

  onMount(() => closeButton?.focus());

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      onClose();
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
      <div class="head">
        <h2 class="title" id={titleId}>
          <span class="badge"><Icon name="triangle-alert" size={16} /></span>
          {notice.text}
        </h2>
        <button type="button" class="close" aria-label={t('relink.failed.closeList')} title={t('relink.failed.closeList')} onclick={onClose}>
          <Icon name="x" size={16} />
        </button>
      </div>
      <p class="body">{body}</p>
      <ul class="list" aria-label={t('relink.failed.list')}>
        {#each rows as row (row.path.join('/'))}
          <li>
            <button
              type="button"
              class="row"
              aria-label={t('relink.failed.open', { name: row.name })}
              onclick={() => onOpen(row.path)}
            >
              <span class="file"><Icon name="file-text" size={16} /></span>
              <span class="where">
                <span class="name">{row.name}</span>
                <span class="folder">{row.folder}</span>
              </span>
              <span class="why">
                <span class="label" class:retried={row.retried}>{row.label}</span>
                <span class="repeat">{row.retried ? t('relink.failed.willRetry') : t('relink.failed.noRepeat')}</span>
              </span>
            </button>
          </li>
        {/each}
      </ul>
    </div>
    <div class="actions">
      <p class="hint">{notice.canRetry ? t('relink.failed.retryHint') : t('relink.failed.noRetryHint')}</p>
      {#if notice.canRetry}
        <button type="button" class="button" onclick={onRetry}>{t('relink.failed.retry')}</button>
      {/if}
      <button type="button" class="button primary" bind:this={closeButton} onclick={onClose}>
        {t('relink.failed.close')}
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
    pointer-events: auto;
  }

  .dialog {
    width: 620px;
    max-width: calc(100vw - 32px);
    max-height: calc(100vh - 32px);
    display: flex;
    flex-direction: column;
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
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 20px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .title {
    display: flex;
    flex: 1;
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

  .close {
    display: grid;
    place-items: center;
    flex: none;
    width: 26px;
    height: 26px;
    border-radius: 5px;
    color: var(--icon-default);
  }

  .close:hover {
    background: var(--bg-hover);
  }

  .body {
    margin: 14px 0 12px;
    color: var(--text-secondary);
    line-height: 20px;
  }

  .list {
    min-height: 0;
    margin: 0;
    padding: 0;
    overflow: auto;
    list-style: none;
    border: 1px solid var(--border-default);
    border-radius: 8px;
  }

  .list li + li {
    border-top: 1px solid var(--border-default);
  }

  .row {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    width: 100%;
    padding: 8px 12px;
    text-align: left;
  }

  .row:hover {
    background: var(--bg-hover);
  }

  .row:focus-visible,
  .close:focus-visible,
  .button:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  .file {
    flex: none;
    padding-top: 2px;
    color: var(--icon-muted);
  }

  .where,
  .why {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .where {
    flex: 1;
  }

  .why {
    flex: none;
    align-items: flex-end;
    max-width: 50%;
    text-align: right;
  }

  .name {
    color: var(--text-primary);
    overflow-wrap: anywhere;
  }

  .folder,
  .repeat {
    color: var(--text-muted);
    font-size: var(--font-size-caption);
  }

  .label {
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .label.retried {
    color: var(--status-warning);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 20px;
    border-top: 1px solid var(--border-default);
    background: var(--bg-sidebar);
  }

  .hint {
    flex: 1;
    margin: 0;
    color: var(--text-muted);
    font-size: var(--font-size-caption);
  }

  .button {
    flex: none;
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
</style>
