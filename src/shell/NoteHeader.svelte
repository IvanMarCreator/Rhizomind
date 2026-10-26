<!--
  Строка над заметкой (К2, решение 2Б): назад/вперёд в общей рамке, путь и имя,
  переключатель режима «книга/карандаш» (решение 72, состояния — Э13). Логика
  режимов — в редакторе (src/editor/live-preview, задача 2.5); здесь только
  выбор кнопки. Переключатель — одна остановка Tab, ← и → выбирают режим;
  подсказка с сочетанием — через ~0,5 с наведения или сразу при фокусе с
  клавиатуры.
-->
<script lang="ts">
  import { onDestroy, type Snippet } from 'svelte';
  import type { EditorMode, VaultPath } from '../api/index.ts';
  import { t, type MessageKey } from '../i18n/index.ts';
  import { chordParts } from '../settings/hotkeys.ts';
  import Icon from './Icon.svelte';

  let {
    path,
    canGoBack,
    canGoForward,
    mode,
    onMode,
    readOnly = false,
    modeShortcut = null,
    onBack,
    onForward,
    status,
  }: {
    path: VaultPath | null;
    canGoBack: boolean;
    canGoForward: boolean;
    mode: EditorMode;
    onMode: (mode: EditorMode) => void;
    /** Заметка только для чтения: оба режима — только просмотр. */
    readOnly?: boolean;
    /** Сочетание режима из реестра горячих клавиш (`Ctrl+KeyE`); `null` — не назначено. */
    modeShortcut?: string | null;
    onBack: () => void;
    onForward: () => void;
    /** Индикатор сохранения (редактор, src/editor/SaveIndicator.svelte). */
    status?: Snippet;
  } = $props();

  const MODES: { mode: EditorMode; label: MessageKey; icon: 'book-open' | 'pencil' }[] = [
    { mode: 'live_preview', label: 'note.mode.livePreview', icon: 'book-open' },
    { mode: 'source', label: 'note.mode.source', icon: 'pencil' },
  ];
  const TOOLTIP_DELAY_MS = 500;

  let buttons: HTMLButtonElement[] = $state([]);
  /** Режим, у кнопки которого видна подсказка. */
  let tooltip = $state<EditorMode | null>(null);
  let tooltipTimer: ReturnType<typeof setTimeout> | undefined;

  function modeLabel(key: MessageKey): string {
    const label = t(key);
    return readOnly ? t('note.mode.readOnly', { mode: label }) : label;
  }

  function showTooltip(next: EditorMode, delay: number): void {
    clearTimeout(tooltipTimer);
    if (delay === 0) tooltip = next;
    else tooltipTimer = setTimeout(() => (tooltip = next), delay);
  }

  function hideTooltip(): void {
    clearTimeout(tooltipTimer);
    tooltip = null;
  }

  function onModeKeydown(event: KeyboardEvent): void {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    event.preventDefault();
    const index = event.key === 'ArrowLeft' ? 0 : 1;
    const next = MODES[index];
    if (!next) return;
    onMode(next.mode);
    buttons[index]?.focus();
  }

  function onFocus(event: FocusEvent, next: EditorMode): void {
    const target = event.currentTarget;
    if (target instanceof HTMLElement && target.matches(':focus-visible')) showTooltip(next, 0);
  }

  onDestroy(() => clearTimeout(tooltipTimer));

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

    {@render status?.()}

    <div class="mode" role="radiogroup" aria-label={t('note.mode.label')}>
      {#each MODES as item, i (item.mode)}
        <button
          bind:this={buttons[i]}
          type="button"
          role="radio"
          class="segment"
          class:pressed={mode === item.mode}
          aria-checked={mode === item.mode}
          aria-label={modeLabel(item.label)}
          aria-keyshortcuts="Control+E"
          tabindex={mode === item.mode ? 0 : -1}
          onclick={() => onMode(item.mode)}
          onkeydown={onModeKeydown}
          onmouseenter={() => showTooltip(item.mode, TOOLTIP_DELAY_MS)}
          onmouseleave={hideTooltip}
          onfocus={(event) => onFocus(event, item.mode)}
          onblur={hideTooltip}
        >
          <Icon name={item.icon} size={15} />
        </button>
      {/each}
      {#if tooltip}
        {@const item = MODES.find((candidate) => candidate.mode === tooltip)}
        <div class="tooltip" aria-hidden="true">
          <span>{item ? modeLabel(item.label) : ''}</span>
          {#if modeShortcut}
            <span class="keys">
              {#each chordParts(modeShortcut) as part, i (i)}
                {#if i > 0}<span class="plus">+</span>{/if}<kbd>{part}</kbd>
              {/each}
            </span>
          {/if}
        </div>
      {/if}
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
    position: relative;
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

  .segment:hover:not(.pressed) {
    background: var(--bg-active);
    color: var(--icon-default);
    cursor: pointer;
  }

  .segment.pressed {
    background: var(--bg-app);
    border-color: var(--border-default);
    color: var(--text-primary);
  }

  .segment:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  /* Подсказка (tokens.md, «Добавлено к решению 72»): под переключателем, справа. */
  .tooltip {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 10;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 5px 8px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg-popover);
    box-shadow: 0 4px 12px rgb(0 0 0 / 14%);
    color: var(--text-primary);
    font-size: var(--font-size-caption);
    white-space: nowrap;
    pointer-events: none;
  }

  .keys {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .plus {
    color: var(--text-faint);
  }

  kbd {
    display: inline-grid;
    place-items: center;
    min-width: 20px;
    height: 20px;
    padding: 0 6px;
    border: 1px solid var(--border-strong);
    border-bottom-width: 2px;
    border-radius: 5px;
    font-family: inherit;
    font-size: var(--font-size-caption);
    font-weight: 500;
    box-sizing: border-box;
  }
</style>
