<!--
  Строка над заметкой (К2, решение 2Б): назад/вперёд в общей рамке, путь и имя,
  переключатель режима «книга/карандаш». Логика режимов — в редакторе (2.5);
  здесь только выбор кнопки.
-->
<script lang="ts">
  import type { EditorMode, VaultPath } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import Icon from './Icon.svelte';

  let {
    path,
    canGoBack,
    canGoForward,
    mode = $bindable(),
    onBack,
    onForward,
  }: {
    path: VaultPath | null;
    canGoBack: boolean;
    canGoForward: boolean;
    mode: EditorMode;
    onBack: () => void;
    onForward: () => void;
  } = $props();

  const folders = $derived(path ? path.slice(0, -1) : []);
  const name = $derived(path ? (path.at(-1) ?? '').replace(/\.md$/i, '') : '');
</script>

<header class="header" aria-label={t('note.header.label')}>
  <div class="history">
    <button
      type="button"
      class="nav"
      disabled={!canGoBack}
      aria-label={t('note.header.back')}
      title={t('note.header.back')}
      onclick={onBack}
    >
      <Icon name="arrow-left" size={15} />
    </button>
    <span class="divider" aria-hidden="true"></span>
    <button
      type="button"
      class="nav"
      disabled={!canGoForward}
      aria-label={t('note.header.forward')}
      title={t('note.header.forward')}
      onclick={onForward}
    >
      <Icon name="arrow-right" size={15} />
    </button>
  </div>

  {#if path}
    <nav class="path" aria-label={t('note.header.path')}>
      {#each folders as folder, i (i)}
        <span class="folder">{folder}</span>
        <span class="separator" aria-hidden="true">/</span>
      {/each}
      <span class="name">{name}</span>
    </nav>

    <div class="mode" role="group" aria-label={t('note.mode.label')}>
      <button
        type="button"
        class="segment"
        class:pressed={mode === 'live_preview'}
        aria-pressed={mode === 'live_preview'}
        aria-label={t('note.mode.livePreview')}
        title={t('note.mode.livePreview')}
        onclick={() => (mode = 'live_preview')}
      >
        <Icon name="book-open" size={15} />
      </button>
      <button
        type="button"
        class="segment"
        class:pressed={mode === 'source'}
        aria-pressed={mode === 'source'}
        aria-label={t('note.mode.source')}
        title={t('note.mode.source')}
        onclick={() => (mode = 'source')}
      >
        <Icon name="pencil" size={15} />
      </button>
    </div>
  {/if}
</header>

<style>
  .header {
    display: flex;
    align-items: center;
    gap: 16px;
    height: var(--note-header-height);
    padding: 0 10px;
    border-bottom: 1px solid var(--border-default);
    box-sizing: border-box;
  }

  .history {
    display: flex;
    align-items: center;
    flex: none;
    border: 1px solid var(--border-default);
    border-radius: 6px;
  }

  .nav {
    display: grid;
    place-items: center;
    width: var(--note-header-button);
    height: var(--note-header-button);
    border-radius: 5px;
    color: var(--icon-default);
  }

  .nav:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .nav:disabled {
    color: var(--icon-muted);
  }

  .divider {
    width: 1px;
    height: 14px;
    background: var(--border-default);
  }

  .path {
    display: flex;
    align-items: baseline;
    gap: 6px;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    color: var(--text-muted);
  }

  .folder {
    flex: 0 1 auto;
    min-width: 2ch;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .separator {
    color: var(--text-faint);
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text-primary);
    font-weight: 500;
  }

  .mode {
    display: flex;
    flex: none;
    margin-left: auto;
    padding: 2px;
    border-radius: 7px;
    background: var(--bg-hover);
  }

  .segment {
    display: grid;
    place-items: center;
    width: var(--note-header-button);
    height: var(--note-header-button);
    border: 1px solid transparent;
    border-radius: 5px;
    color: var(--icon-muted);
    box-sizing: border-box;
  }

  .segment.pressed {
    background: var(--bg-app);
    border-color: var(--border-default);
    color: var(--icon-default);
  }
</style>
