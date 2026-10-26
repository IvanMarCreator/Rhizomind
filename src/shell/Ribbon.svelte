<!-- Лента иконок слева (К2, решение 1А): сверху «Файлы» и «Быстрый переход», снизу «Настройки». -->
<script lang="ts">
  import { t } from '../i18n/index.ts';
  import Icon from './Icon.svelte';

  let {
    filesOpen,
    onToggleFiles,
    onQuickSwitcher,
    onSettings,
  }: {
    filesOpen: boolean;
    onToggleFiles: () => void;
    /** Быстрый переход — задача 2.7. */
    onQuickSwitcher?: () => void;
    /** Настройки хранилища — задача 2.8. */
    onSettings?: () => void;
  } = $props();
</script>

<div class="ribbon" role="toolbar" aria-label={t('shell.ribbon.label')} aria-orientation="vertical">
  <div class="group">
    <button
      type="button"
      class="action"
      class:active={filesOpen}
      aria-pressed={filesOpen}
      aria-label={t('shell.ribbon.files')}
      title={t('shell.ribbon.files')}
      onclick={onToggleFiles}
    >
      <Icon name="files" />
    </button>
    <button
      type="button"
      class="action"
      aria-label={t('shell.ribbon.quickSwitcher')}
      title={t('shell.ribbon.quickSwitcher')}
      onclick={() => onQuickSwitcher?.()}
    >
      <Icon name="square-arrow-right" />
    </button>
  </div>
  <div class="group">
    <!-- Окно настроек — после КТ1 (задача 2.8б): до него кнопка явно
         неактивна, с подсказкой, чтобы не казалось, что программа сломана. -->
    <button
      type="button"
      class="action"
      class:unavailable={!onSettings}
      aria-label={t('shell.ribbon.settings')}
      aria-disabled={onSettings ? undefined : 'true'}
      title={onSettings ? t('shell.ribbon.settings') : t('shell.ribbon.settingsLater')}
      onclick={() => onSettings?.()}
    >
      <Icon name="settings" />
    </button>
  </div>
</div>

<style>
  .ribbon {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    align-items: center;
    width: var(--ribbon-width);
    padding: 8px 0;
    background: var(--bg-ribbon);
    border-right: 1px solid var(--border-default);
    box-sizing: border-box;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .action {
    display: grid;
    place-items: center;
    width: var(--ribbon-button);
    height: var(--ribbon-button);
    border-radius: 5px;
    color: var(--icon-default);
  }

  .action:hover {
    background: var(--bg-hover);
  }

  .action.unavailable {
    color: var(--icon-muted);
    opacity: 0.55;
    cursor: default;
  }

  .action.unavailable:hover {
    background: none;
  }

  .action.active {
    background: var(--bg-active);
    color: var(--text-primary);
  }
</style>
