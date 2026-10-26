<!-- Лента иконок слева (К2, решение 1А): сверху «Файлы» и «Быстрый переход», снизу «Настройки». -->
<script lang="ts">
  import { t } from '../i18n/index.ts';
  import Icon from './Icon.svelte';

  let {
    filesOpen,
    onToggleFiles,
    onQuickSwitcher,
    quickSwitcherShortcut = null,
    onSettings,
  }: {
    filesOpen: boolean;
    onToggleFiles: () => void;
    /** Быстрый переход — задача 2.7. */
    onQuickSwitcher?: () => void;
    /** Сочетание быстрого перехода для подсказки (переназначается, решение 72); `null` — нет. */
    quickSwitcherShortcut?: string | null;
    /** Окно «Настройки хранилища» — задача 2.8б. */
    onSettings: () => void;
  } = $props();

  const quickSwitcherLabel = $derived(
    quickSwitcherShortcut
      ? t('shell.ribbon.quickSwitcherWith', { shortcut: quickSwitcherShortcut })
      : t('shell.ribbon.quickSwitcher'),
  );
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
      aria-label={quickSwitcherLabel}
      title={quickSwitcherLabel}
      onclick={() => onQuickSwitcher?.()}
    >
      <Icon name="square-arrow-right" />
    </button>
  </div>
  <div class="group">
    <button
      type="button"
      class="action"
      aria-label={t('shell.ribbon.settings')}
      title={t('shell.ribbon.settings')}
      onclick={onSettings}
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

  .action.active {
    background: var(--bg-active);
    color: var(--text-primary);
  }
</style>
