<!--
  Индикатор в строке над заметкой (макеты Э1 и Э9): «Сохранено» с галочкой,
  «Сохраняется», «Не сохранено», «Только чтение» с замком. Живая область
  объявляет только беду (не сохранено, нет файла, удалена), а не каждую паузу
  ввода. Подробность ошибки — у кнопки-индикатора, доступной с клавиатуры.
-->
<script lang="ts">
  import { userMessage } from '../i18n/errors.ts';
  import { t, type MessageKey } from '../i18n/index.ts';
  import type { SaveStatus } from './session.ts';
  import StatusIcon, { type StatusIconName } from './StatusIcon.svelte';

  let { status }: { status: SaveStatus | null } = $props();

  const LOOK: Record<SaveStatus['kind'], { text: MessageKey; icon: StatusIconName | null; alert: boolean }> = {
    saved: { text: 'editor.status.saved', icon: 'check', alert: false },
    pending: { text: 'editor.status.pending', icon: null, alert: false },
    failed: { text: 'editor.status.failed', icon: 'circle-alert', alert: true },
    missing: { text: 'editor.status.missing', icon: 'circle-alert', alert: true },
    read_only: { text: 'editor.status.readOnly', icon: 'lock', alert: false },
    deleted: { text: 'editor.status.deleted', icon: 'circle-alert', alert: true },
  };

  const look = $derived(status ? LOOK[status.kind] : null);
  const detail = $derived(status?.kind === 'failed' ? userMessage(status.error) : null);
  const announce = $derived(look?.alert ? t(look.text) : '');
  const detailId = $props.id();
</script>

<span class="live" role="status" aria-label={t('editor.status.label')}>{announce}</span>

{#if status && look}
  {#if detail}
    <button
      type="button"
      class="status {status.kind}"
      data-testid="save-status"
      title={detail}
      aria-describedby={detailId}
    >
      {#if look.icon}<StatusIcon name={look.icon} />{/if}
      <span>{t(look.text)}</span>
    </button>
    <span id={detailId} class="live">{detail}</span>
  {:else}
    <span class="status {status.kind}" data-testid="save-status">
      {#if look.icon}<StatusIcon name={look.icon} />{/if}
      <span>{t(look.text)}</span>
    </span>
  {/if}
{/if}

<style>
  .status {
    display: flex;
    align-items: center;
    gap: 5px;
    flex: none;
    padding: 0;
    border: none;
    background: none;
    font: inherit;
    font-size: var(--font-size-caption);
    color: var(--text-muted);
    white-space: nowrap;
  }

  button.status {
    border-radius: 4px;
  }

  button.status:focus-visible {
    box-shadow: var(--focus-ring);
    outline: none;
  }

  /* Видно только программам чтения с экрана. */
  .live {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  .saved :global(svg) {
    color: var(--status-saved);
  }

  .read_only {
    color: var(--status-warning);
  }

  .failed,
  .missing,
  .deleted {
    color: var(--status-error);
  }
</style>
