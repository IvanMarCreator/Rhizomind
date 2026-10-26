<!-- Раздел «Оформление» (Э12.4): тема с мини-превью. -->
<script lang="ts">
  import type { Settings, Theme } from '../../api/index.ts';
  import { t } from '../../i18n/index.ts';

  let { settings, onChange }: { settings: Settings; onChange: (patch: Partial<Settings>) => void } = $props();

  const themes: readonly { value: Theme; label: string }[] = [
    { value: 'system', label: t('settings.theme.system') },
    { value: 'light', label: t('settings.theme.light') },
    { value: 'dark', label: t('settings.theme.dark') },
  ];
</script>

<div class="row">
  <div class="title" id="settings-theme">{t('settings.theme.title')}</div>
  <p class="hint">{t('settings.theme.hint')}</p>
  <div class="themes" role="radiogroup" aria-labelledby="settings-theme">
    {#each themes as theme (theme.value)}
      <button
        type="button"
        role="radio"
        class="theme"
        aria-checked={settings.theme === theme.value}
        onclick={() => onChange({ theme: theme.value })}
      >
        <span class="preview {theme.value}" aria-hidden="true">
          <span class="half light"><span class="bar"></span><span class="line"></span><span class="line"></span></span>
          <span class="half dark"><span class="bar"></span><span class="line"></span><span class="line"></span></span>
        </span>
        <span class="label"><span class="dot"></span>{theme.label}</span>
      </button>
    {/each}
  </div>
</div>

<style>
  .row {
    padding: 14px 0;
    border-bottom: 1px solid var(--border-default);
  }

  .title {
    color: var(--text-primary);
    font-weight: 500;
  }

  .hint {
    margin: 3px 0 12px;
    color: var(--text-muted);
    font-size: var(--font-size-caption);
  }

  .themes {
    display: flex;
    gap: 10px;
  }

  .theme {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 132px;
    text-align: left;
  }

  .preview {
    display: flex;
    height: 76px;
    overflow: hidden;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
  }

  .theme[aria-checked='true'] .preview {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }

  .theme:focus-visible {
    outline: none;
  }

  .theme:focus-visible .preview {
    box-shadow: var(--focus-ring);
  }

  .half {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 6px;
    padding: 10px 8px;
  }

  /* Превью рисуют постоянные цвета светлой и тёмной темы, а не переменные текущей. */
  .half.light {
    background: #ffffff;
  }

  .half.dark {
    background: #1c1d20;
  }

  .preview.light .half.dark,
  .preview.dark .half.light {
    display: none;
  }

  .bar,
  .line {
    display: block;
    height: 3px;
    border-radius: 2px;
  }

  .light .bar {
    width: 60%;
    background: #1f2328;
  }

  .light .line {
    background: #cdd1d6;
  }

  .dark .bar {
    width: 60%;
    background: #e4e6e9;
  }

  .dark .line {
    background: #3a3d42;
  }

  .label {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-primary);
    font-size: var(--font-size-caption);
  }

  .dot {
    width: 10px;
    height: 10px;
    border: 1px solid var(--border-strong);
    border-radius: 50%;
    box-sizing: border-box;
  }

  .theme[aria-checked='true'] .dot {
    border: 3px solid var(--accent);
  }
</style>
