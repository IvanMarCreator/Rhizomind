<!--
  Зона уведомлений внизу окна (Э7): справа снизу, новые — ниже старых.
  Под обычными — уведомления об итоге обновления ссылок (0.2-5.1, макет У-1):
  значок, текст, приглушённая вторая строка, кнопки «Показать» / «Повторить»;
  сами не исчезают (решение 121). «Показать» открывает список У-3.
-->
<script lang="ts">
  import type { VaultPath } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import FailedList from '../relink/FailedList.svelte';
  import type { RelinkNotices, RelinkNoticesState } from '../relink/notices.ts';
  import Icon from './Icon.svelte';
  import type { Notice } from './window.svelte.ts';

  let {
    notices,
    onDismiss,
    relink,
    onOpenNote,
  }: {
    notices: Notice[];
    onDismiss: (id: number) => void;
    /** Уведомления об обновлении ссылок; нет — только обычные. */
    relink?: RelinkNotices | undefined;
    /** Щелчок по заметке в списке «Не удалось обновить ссылки». */
    onOpenNote?: ((path: VaultPath) => void) | undefined;
  } = $props();

  let relinkState = $state.raw<RelinkNoticesState>({ notices: [], shown: null });
  $effect(() => relink?.subscribe((next) => (relinkState = next)));

  function openNote(path: VaultPath) {
    relink?.hide();
    onOpenNote?.(path);
  }
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
  {#each relinkState.notices as notice (notice.id)}
    <div class="notice" data-tone={notice.tone}>
      <span class="mark">
        {#if notice.tone === 'success'}
          <Icon name="check" size={16} />
        {:else if notice.tone === 'warning'}
          <Icon name="triangle-alert" size={16} />
        {:else}
          <Icon name="loader-circle" size={16} />
        {/if}
      </span>
      <div class="main">
        <p class="text">{notice.text}</p>
        {#if notice.detail}<p class="detail">{notice.detail}</p>{/if}
        {#if notice.canShow || notice.canRetry}
          <div class="buttons">
            {#if notice.canShow}
              <button type="button" class="button" onclick={() => relink?.show(notice.id)}>{t('relink.failed.show')}</button>
            {/if}
            {#if notice.canRetry}
              <button type="button" class="button" onclick={() => void relink?.retry(notice.id)}>
                {t('relink.failed.retry')}
              </button>
            {/if}
          </div>
        {/if}
      </div>
      {#if notice.closable}
        <button
          type="button"
          class="close"
          aria-label={t('notifications.close')}
          title={t('notifications.close')}
          onclick={() => relink?.dismiss(notice.id)}
        >
          <Icon name="x" size={14} />
        </button>
      {/if}
    </div>
  {/each}
</section>

{#if relink && relinkState.shown}
  {@const shown = relinkState.shown}
  <FailedList
    notice={shown}
    limitBytes={relink.limitBytes}
    onOpen={openNote}
    onRetry={() => void relink?.retry(shown.id)}
    onClose={() => relink?.hide()}
  />
{/if}

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

  .mark {
    display: grid;
    flex: none;
    padding-top: 1px;
    color: var(--icon-default);
  }

  [data-tone='success'] .mark {
    color: var(--status-saved);
  }

  [data-tone='warning'] .mark {
    color: var(--status-warning);
  }

  .main {
    flex: 1;
    min-width: 0;
  }

  .text {
    flex: 1;
    margin: 0;
    color: var(--text-primary);
    line-height: 18px;
  }

  .detail {
    margin: 4px 0 0;
    color: var(--text-muted);
    font-size: var(--font-size-caption);
    line-height: 16px;
  }

  .buttons {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }

  .button {
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg-app);
    color: var(--text-primary);
    font-weight: 500;
  }

  .button:hover {
    background: var(--bg-hover);
  }

  .button:focus-visible,
  .close:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
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
