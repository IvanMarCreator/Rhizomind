<!--
  Подсказка «только чтение» над текстом заметки (макет Э9): почему файл не
  редактируется и что сделать. Кнопка «Открыть настройки» — когда окно
  настроек подключено (задача 2.8).
-->
<script lang="ts">
  import type { ReadOnlyReason } from '../api/index.ts';
  import { LIMITS } from '../api/limits.ts';
  import { t } from '../i18n/index.ts';
  import StatusIcon from './StatusIcon.svelte';

  let {
    reason,
    size,
    onOpenSettings,
  }: { reason: ReadOnlyReason; size: number; onOpenSettings?: (() => void) | undefined } = $props();

  const MIB = 1024 * 1024;
  const number = new Intl.NumberFormat('ru', { maximumFractionDigits: 1 });
  const megabytes = (bytes: number) => t('units.megabytes', { value: number.format(bytes / MIB) });

  const title = $derived(
    reason.reason === 'too_large'
      ? t('editor.readOnly.tooLarge.title', { size: megabytes(size), limit: megabytes(reason.limit_bytes) })
      : t('editor.readOnly.invalidUtf8.title'),
  );
  const hint = $derived(
    reason.reason === 'too_large' ? t('editor.readOnly.tooLarge.hint') : t('editor.readOnly.invalidUtf8.hint'),
  );
  /** Больше жёсткого потолка ядро текст не читает (contracts/v1/note_read.snap). */
  const hidden = $derived(reason.reason === 'too_large' && size > LIMITS.max_note_bytes);
</script>

<div class="notice">
  <span class="icon"><StatusIcon name="lock" size={16} /></span>
  <div class="text">
    <p class="title">{title}</p>
    <p class="hint">{hint}</p>
    {#if hidden}
      <p class="hint">{t('editor.readOnly.tooLarge.hidden', { max: megabytes(LIMITS.max_note_bytes) })}</p>
    {/if}
  </div>
  {#if onOpenSettings && reason.reason === 'too_large'}
    <button type="button" class="settings" onclick={onOpenSettings}>{t('editor.readOnly.openSettings')}</button>
  {/if}
</div>

<style>
  .notice {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    margin-bottom: 28px;
    padding: 12px 14px;
    border: 1px solid color-mix(in srgb, var(--status-warning) 45%, transparent);
    border-radius: 8px;
    background: var(--status-warning-subtle);
  }

  .icon {
    padding-top: 1px;
    color: var(--status-warning);
  }

  .text {
    flex: 1;
    min-width: 0;
  }

  p {
    margin: 0;
  }

  .title {
    color: var(--text-primary);
    font-size: var(--font-size-caption);
    font-weight: 600;
    line-height: 18px;
  }

  .hint {
    margin-top: 2px;
    color: var(--text-secondary);
    font-size: 11px;
    line-height: 16px;
  }

  .settings {
    flex: none;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg-app);
    color: var(--text-primary);
    font-size: 11px;
    font-weight: 500;
  }

  .settings:hover {
    background: var(--bg-hover);
  }

  .settings:focus-visible {
    box-shadow: var(--focus-ring);
    outline: none;
  }
</style>
