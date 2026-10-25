<!-- Зона уведомлений внизу окна (Э7): справа снизу, новые — ниже старых. -->
<script lang="ts">
  import { t } from '../i18n/index.ts';
  import Icon from './Icon.svelte';
  import type { Notice } from './window.svelte.ts';

  let { notices, onDismiss }: { notices: Notice[]; onDismiss: (id: number) => void } = $props();
</script>

<section class="notifications" aria-label={t('notifications.label')} aria-live="polite">
  {#each notices as notice (notice.id)}
    <div class="notice">
      <p class="text">{notice.text}</p>
      <button
        type="button"
        class="close"
        aria-label={t('notifications.close')}
        title={t('notifications.close')}
        onclick={() => onDismiss(notice.id)}
      >
        <Icon name="x" size={14} />
      </button>
    </div>
  {/each}
</section>

<style>
  .notifications {
    position: fixed;
    right: 20px;
    bottom: 20px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: min(var(--notification-width), calc(100vw - 40px));
    pointer-events: none;
  }

  .notice {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 14px;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    background: var(--bg-popover);
    box-shadow: 0 6px 20px rgb(0 0 0 / 12%);
    pointer-events: auto;
  }

  .text {
    flex: 1;
    margin: 0;
    color: var(--text-primary);
    line-height: 18px;
  }

  .close {
    display: grid;
    place-items: center;
    flex: none;
    width: 22px;
    height: 22px;
    margin: -2px -4px 0 0;
    border-radius: 5px;
    color: var(--icon-default);
  }

  .close:hover {
    background: var(--bg-hover);
  }
</style>
