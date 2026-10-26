<!--
  Раздел «Горячие клавиши» (решение 72, макеты Э12.5б–г): команды реестра и
  их сочетания. Клик по сочетанию — запись нового вместо него, «+» — ещё
  одно, «×» — убрать, «↺» — стандартные, «Сбросить все». Запись: первое
  нажатое допустимое сочетание; Esc или клик мимо — отмена. Занятое другой
  командой — выбор «Заменить / Отмена» (Enter и Esc — «Отмена», решение 73).
  Системные сочетания показаны с замком и не переназначаются.
-->
<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { HotkeyOverrides } from '../../api/index.ts';
  import { LIMITS } from '../../api/limits.ts';
  import { t } from '../../i18n/index.ts';
  import Icon from '../../shell/Icon.svelte';
  import {
    COMMANDS,
    SYSTEM_ROWS,
    assignChord,
    bindingsOf,
    checkChord,
    chordFromEvent,
    chordText,
    isChanged,
    ownerOf,
    removeChord,
    resetCommand,
    type CommandId,
    type HotkeyCommand,
  } from '../hotkeys.ts';
  import Keys from '../Keys.svelte';

  let { hotkeys, onChange }: { hotkeys: HotkeyOverrides; onChange: (hotkeys: HotkeyOverrides) => void } = $props();

  /** Запись: команда и сочетание, вместо которого пишется новое (`null` — ещё одно). */
  let recording = $state<{ id: CommandId; replacing: string | null } | null>(null);
  /** Нажатое сочетание занято другой командой: ждёт «Заменить / Отмена». */
  let conflict = $state<{ id: CommandId; replacing: string | null; chord: string; owner: HotkeyCommand } | null>(null);
  /** Почему нажатое сочетание не записано (системное, без Ctrl/Alt). */
  let problem = $state<string | null>(null);
  let filter = $state('');
  let recorder: HTMLElement | undefined = $state();
  let cancelButton: HTMLButtonElement | undefined = $state();

  const needle = $derived(filter.trim().toLocaleLowerCase('ru'));
  const matches = (label: string) => !needle || label.toLocaleLowerCase('ru').includes(needle);
  const commands = $derived(COMMANDS.filter((command) => matches(t(command.label))));
  const systemRows = $derived(SYSTEM_ROWS.filter((row) => matches(t(row.label))));

  onMount(() => {
    // Перехват на окне: запись получает нажатие раньше окна настроек (Esc не
    // закрывает его) и раньше горячих клавиш окна.
    window.addEventListener('keydown', onWindowKeydown, true);
    document.addEventListener('pointerdown', onPointerDown, true);
    return () => {
      window.removeEventListener('keydown', onWindowKeydown, true);
      document.removeEventListener('pointerdown', onPointerDown, true);
    };
  });

  function startRecording(id: CommandId, replacing: string | null) {
    conflict = null;
    problem = null;
    recording = { id, replacing };
  }

  function stop() {
    recording = null;
    conflict = null;
    problem = null;
  }

  function onWindowKeydown(event: KeyboardEvent) {
    if (conflict) {
      if (event.key === 'Escape' || (event.key === 'Enter' && !(event.target instanceof HTMLButtonElement))) {
        event.preventDefault();
        event.stopPropagation();
        stop();
      }
      return;
    }
    if (!recording) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === 'Escape') {
      stop();
      return;
    }
    const chord = chordFromEvent(event);
    if (!chord) return; // Пока нажат только модификатор — ждём клавишу.
    const found = checkChord(chord);
    if (found) {
      problem = t(`settings.hotkeys.problem.${found}`);
      return;
    }
    const { id, replacing } = recording;
    const owner = ownerOf(hotkeys, chord, id);
    if (owner) {
      recording = null;
      problem = null;
      conflict = { id, replacing, chord, owner };
      void tick().then(() => cancelButton?.focus());
      return;
    }
    stop();
    if (!bindingsOf(hotkeys, id).includes(chord)) onChange(assignChord(hotkeys, id, chord, replacing));
  }

  function onPointerDown(event: PointerEvent) {
    if (recording && recorder && !(event.target instanceof Node && recorder.contains(event.target))) stop();
  }

  function replace() {
    if (!conflict) return;
    const { id, chord, replacing } = conflict;
    stop();
    onChange(assignChord(hotkeys, id, chord, replacing));
  }

  function resetAll() {
    stop();
    let next = hotkeys;
    for (const command of COMMANDS) next = resetCommand(next, command.id);
    onChange(next);
  }

  const isRecording = (id: CommandId, replacing: string | null) =>
    recording?.id === id && recording.replacing === replacing;
  const inConflict = (id: CommandId, replacing: string | null) =>
    conflict?.id === id && conflict.replacing === replacing;
</script>

{#snippet recordField()}
  <span class="recorder" bind:this={recorder} data-testid="hotkey-recorder">
    {t('settings.hotkeys.recordPrompt')}<span class="cancel-hint">{t('settings.hotkeys.recordCancel')}</span>
  </span>
{/snippet}

{#snippet conflictField(chord: string)}
  <span class="conflict-chord"><Keys {chord} /><span class="cancel-hint">{t('settings.hotkeys.recordCancel')}</span></span>
{/snippet}

<div class="toolbar">
  <label class="search">
    <Icon name="search" size={14} />
    <input bind:value={filter} placeholder={t('settings.hotkeys.search')} aria-label={t('settings.hotkeys.search')} />
  </label>
  <button type="button" class="button" onclick={resetAll}>
    <Icon name="rotate-ccw" size={13} />{t('settings.hotkeys.resetAll')}
  </button>
</div>

<ul class="commands" aria-label={t('settings.section.hotkeys')}>
  {#each commands as command (command.id)}
    {@const chords = bindingsOf(hotkeys, command.id)}
    {@const changed = isChanged(hotkeys, command.id)}
    {@const label = t(command.label)}
    <li class="command" data-command={command.id}>
      <div class="line">
        <div class="name">
          <div class="label">
            <span>{label}</span>
            {#if changed}<span class="changed">{t('settings.hotkeys.changed')}</span>{/if}
          </div>
          {#if recording?.id === command.id}
            <div class="status recording">
              {problem ??
                (recording.replacing
                  ? t('settings.hotkeys.recording', { chord: chordText(recording.replacing) })
                  : t('settings.hotkeys.recordingNew'))}
            </div>
          {:else if conflict?.id === command.id}
            <div class="status busy">{t('settings.hotkeys.busy')}</div>
          {:else if command.hint}
            <div class="status">{t(command.hint)}</div>
          {/if}
        </div>
        <div class="chords">
          {#each chords as chord (chord)}
            {#if isRecording(command.id, chord)}
              {@render recordField()}
            {:else if inConflict(command.id, chord) && conflict}
              {@render conflictField(conflict.chord)}
            {:else}
              <span class="chip">
                <button
                  type="button"
                  class="chord"
                  aria-label={t('settings.hotkeys.edit', { chord: chordText(chord) })}
                  onclick={() => startRecording(command.id, chord)}
                >
                  <Keys {chord} />
                </button>
                <button
                  type="button"
                  class="remove"
                  aria-label={t('settings.hotkeys.remove', { chord: chordText(chord) })}
                  title={t('settings.hotkeys.remove', { chord: chordText(chord) })}
                  onclick={() => onChange(removeChord(hotkeys, command.id, chord))}
                >
                  <Icon name="x" size={11} />
                </button>
              </span>
            {/if}
          {:else}
            {#if !isRecording(command.id, null) && !inConflict(command.id, null)}
              <span class="none">{t('settings.hotkeys.none')}</span>
            {/if}
          {/each}
          {#if isRecording(command.id, null)}
            {@render recordField()}
          {:else if inConflict(command.id, null) && conflict}
            {@render conflictField(conflict.chord)}
          {/if}
          <button
            type="button"
            class="icon-button"
            aria-label={t('settings.hotkeys.add', { command: label })}
            title={t('settings.hotkeys.add', { command: label })}
            disabled={chords.length >= LIMITS.hotkey_max_chords}
            onclick={() => startRecording(command.id, null)}
          >
            <Icon name="plus" size={14} />
          </button>
          <span class="reset-slot">
            {#if changed}
              <button
                type="button"
                class="icon-button"
                aria-label={t('settings.hotkeys.reset', { command: label })}
                title={t('settings.hotkeys.reset', { command: label })}
                onclick={() => {
                  stop();
                  onChange(resetCommand(hotkeys, command.id));
                }}
              >
                <Icon name="rotate-ccw" size={13} />
              </button>
            {/if}
          </span>
        </div>
      </div>
      {#if conflict?.id === command.id}
        <div class="conflict" role="alertdialog" aria-label={t('settings.hotkeys.busy')}>
          <Icon name="triangle-alert" size={15} />
          <div class="conflict-text">
            <div class="conflict-title">
              {t('settings.hotkeys.conflict.title', { chord: chordText(conflict.chord), command: t(conflict.owner.label) })}
            </div>
            <div>{t('settings.hotkeys.conflict.body', { command: t(conflict.owner.label) })}</div>
          </div>
          <button type="button" class="button" bind:this={cancelButton} onclick={stop}>
            {t('settings.hotkeys.conflict.cancel')}
          </button>
          <button type="button" class="button primary" onclick={replace}>{t('settings.hotkeys.conflict.replace')}</button>
        </div>
      {/if}
    </li>
  {/each}
  {#each systemRows as row (row.label)}
    <li class="command system">
      <div class="line">
        <div class="name">
          <div class="label">
            <span>{t(row.label)}</span><span class="system-badge">{t('settings.hotkeys.system')}</span>
          </div>
        </div>
        <div class="chords">
          {#each row.chords as chord (chord)}<Keys {chord} muted />{/each}
          <span class="lock" title={t('settings.hotkeys.locked')}><Icon name="lock" size={13} /></span>
          <span class="reset-slot"></span>
        </div>
      </div>
    </li>
  {/each}
  {#if commands.length === 0 && systemRows.length === 0}
    <li class="empty">{t('settings.hotkeys.empty')}</li>
  {/if}
</ul>

<style>
  .toolbar {
    display: flex;
    gap: 8px;
    margin-bottom: 8px;
  }

  .search {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 10px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    color: var(--icon-muted);
  }

  .search:focus-within {
    border-color: var(--accent);
    box-shadow: var(--focus-ring);
  }

  .search input {
    flex: 1;
    min-width: 0;
    border: none;
    background: none;
    color: var(--text-primary);
    font: inherit;
    outline: none;
  }

  .button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 12px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg-app);
    color: var(--text-primary);
    white-space: nowrap;
  }

  .button:hover {
    background: var(--bg-hover);
  }

  .button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--text-on-accent);
  }

  .button:focus-visible,
  .icon-button:focus-visible,
  .chord:focus-visible,
  .remove:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  .commands {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .command {
    padding: 10px 0;
    border-bottom: 1px solid var(--border-default);
  }

  .line {
    display: flex;
    align-items: center;
    gap: 16px;
    min-height: 26px;
  }

  .name {
    flex: 1;
    min-width: 0;
  }

  .label {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-primary);
  }

  .status {
    margin-top: 2px;
    color: var(--text-muted);
    font-size: var(--font-size-caption);
  }

  .status.recording {
    color: var(--accent-text);
  }

  .status.busy {
    color: var(--status-warning);
  }

  .changed {
    padding: 1px 6px;
    border-radius: 8px;
    background: var(--accent-subtle);
    color: var(--accent-text);
    font-size: 11px;
  }

  .system-badge {
    padding: 1px 6px;
    border-radius: 8px;
    background: var(--bg-active);
    color: var(--text-muted);
    font-size: 11px;
  }

  .system .label {
    color: var(--text-secondary);
  }

  .chords {
    display: flex;
    flex: none;
    align-items: center;
    gap: 6px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    border-radius: 5px;
  }

  .chord {
    padding: 2px;
    border-radius: 5px;
  }

  .chip:hover {
    background: var(--bg-hover);
  }

  .remove {
    display: grid;
    width: 0;
    height: 18px;
    overflow: hidden;
    place-items: center;
    border-radius: 4px;
    color: var(--icon-muted);
  }

  .chip:hover .remove,
  .remove:focus-visible {
    width: 18px;
  }

  .remove:hover {
    color: var(--text-primary);
  }

  .recorder,
  .conflict-chord {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--accent);
    border-radius: 5px;
    color: var(--text-muted);
    font-size: var(--font-size-caption);
    box-shadow: 0 0 0 2px var(--accent-subtle);
  }

  .conflict-chord {
    border-color: var(--status-warning);
    box-shadow: 0 0 0 2px var(--status-warning-subtle);
  }

  .cancel-hint {
    color: var(--text-faint);
    font-size: 11px;
  }

  .none {
    color: var(--text-faint);
    font-size: var(--font-size-caption);
  }

  .icon-button {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 4px;
    color: var(--icon-default);
  }

  .icon-button:hover {
    background: var(--bg-hover);
  }

  .icon-button:disabled {
    visibility: hidden;
  }

  .reset-slot {
    display: inline-flex;
    width: 22px;
  }

  .lock {
    display: grid;
    place-items: center;
    width: 22px;
    color: var(--icon-muted);
  }

  .conflict {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 10px;
    padding: 10px 12px;
    border: 1px solid var(--status-warning);
    border-radius: 6px;
    background: var(--status-warning-subtle);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .conflict :global(.icon) {
    flex: none;
    color: var(--status-warning);
  }

  .conflict-text {
    flex: 1;
    min-width: 0;
  }

  .conflict-title {
    color: var(--text-primary);
    font-weight: 500;
  }

  .empty {
    padding: 14px 0;
    color: var(--text-muted);
  }
</style>
