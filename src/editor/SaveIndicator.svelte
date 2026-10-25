<!--
  Индикатор в строке над заметкой (макеты Э1 и Э9): «Сохранено» с галочкой,
  «Сохраняется», «Не сохранено», «Только чтение» с замком.
-->
<script lang="ts">
  import { userMessage } from '../i18n/errors.ts';
  import { t, type MessageKey } from '../i18n/index.ts';
  import type { SaveStatus } from './session.ts';
  import StatusIcon, { type StatusIconName } from './StatusIcon.svelte';

  let { status }: { status: SaveStatus | null } = $props();

  const LOOK: Record<SaveStatus['kind'], { text: MessageKey; icon: StatusIconName | null }> = {
    saved: { text: 'editor.status.saved', icon: 'check' },
    pending: { text: 'editor.status.pending', icon: null },
    failed: { text: 'editor.status.failed', icon: 'circle-alert' },
    read_only: { text: 'editor.status.readOnly', icon: 'lock' },
    deleted: { text: 'editor.status.deleted', icon: 'circle-alert' },
  };

  const look = $derived(status ? LOOK[status.kind] : null);
  const detail = $derived(status?.kind === 'failed' ? userMessage(status.error) : undefined);
</script>

{#if status && look}
  <span class="status {status.kind}" role="status" aria-label={t('editor.status.label')} title={detail}>
    {#if look.icon}<StatusIcon name={look.icon} />{/if}
    <span>{t(look.text)}</span>
  </span>
{/if}

<style>
  .status {
    display: flex;
    align-items: center;
    gap: 5px;
    flex: none;
    font-size: var(--font-size-caption);
    color: var(--text-muted);
    white-space: nowrap;
  }

  .saved :global(svg) {
    color: var(--status-saved);
  }

  .read_only {
    color: var(--status-warning);
  }

  .failed,
  .deleted {
    color: var(--status-error);
  }
</style>
