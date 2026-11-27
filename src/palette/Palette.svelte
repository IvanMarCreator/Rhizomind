<script lang="ts">
  import { onMount } from 'svelte';
  import type { HotkeyOverrides } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import { COMMANDS, bindingsOf, chordText, type CommandId } from '../settings/hotkeys.ts';
  import Icon from '../shell/Icon.svelte';

  let { hotkeys, available, onChoose, onCancel }: {
    hotkeys: HotkeyOverrides;
    available: (id: CommandId) => boolean;
    onChoose: (id: CommandId) => void;
    onCancel: () => void;
  } = $props();
  const uid = $props.id();
  let input: HTMLInputElement;
  let query = $state('');
  let highlighted = $state<CommandId | null>(null);
  const needle = $derived(query.trim().toLocaleLowerCase('ru'));
  const matches = $derived(COMMANDS.filter((c) => c.id !== 'palette.open' && t(c.label).toLocaleLowerCase('ru').includes(needle)));
  const enabled = $derived(matches.filter((c) => available(c.id)));
  const current = $derived(enabled.find((c) => c.id === highlighted) ?? enabled[0]);

  onMount(() => {
    input.focus();
    // Фоновое обновление дерева или редактора может вернуть туда фокус.
    // Меню правки поля поиска при закрытии само вернёт его в поле.
    function keepFocus(event: FocusEvent) {
      if (event.target instanceof Element && event.target !== input && !event.target.closest('[role="menu"]')) {
        input.focus();
      }
    }
    document.addEventListener('focusin', keepFocus, true);
    return () => document.removeEventListener('focusin', keepFocus, true);
  });

  $effect(() => {
    if (current) document.getElementById(`${uid}-${current.id}`)?.scrollIntoView?.({ block: 'nearest' });
  });

  function keydown(event: KeyboardEvent) {
    if (event.isComposing) return;
    switch (event.key) {
      case 'ArrowUp':
      case 'ArrowDown': {
        event.preventDefault();
        const index = current ? enabled.indexOf(current) : -1;
        const step = event.key === 'ArrowUp' ? -1 : 1;
        highlighted = enabled[Math.max(0, Math.min(enabled.length - 1, index + step))]?.id ?? null;
        break;
      }
      case 'Enter':
        event.preventDefault();
        if (!event.repeat && current) onChoose(current.id);
        break;
      case 'Escape':
        event.preventDefault();
        onCancel();
        break;
      case 'Tab':
        // Единственная точка фокуса; варианты озвучиваются через aria-activedescendant.
        event.preventDefault();
        break;
    }
  }
</script>

<!-- С клавиатуры за отмену отвечает Escape, снаружи окна — щелчок по фону. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" onmousedown={(event) => { if (event.target === event.currentTarget) onCancel(); }}>
  <div role="dialog" aria-modal="true" aria-labelledby={uid + '-title'} tabindex="-1" class="palette"
    onmousedown={(event) => { if (event.target !== input) event.preventDefault(); }}>
    <h2 id={uid + '-title'}>{t('palette.title')}</h2>
    <div class="field">
      <Icon name="search" size={18} />
      <input bind:this={input} role="combobox" aria-label={t('palette.placeholder')} aria-expanded="true"
        aria-controls={uid + '-list'} aria-autocomplete="list"
        aria-activedescendant={current && `${uid}-${current.id}`}
        placeholder={t('palette.placeholder')} autocomplete="off" spellcheck="false"
        value={query} oninput={(event) => { query = event.currentTarget.value; highlighted = null; }} onkeydown={keydown} />
    </div>
    <ul id={uid + '-list'} role="listbox" aria-label={t('palette.commands')}>
      {#each matches as command (command.id)}
        {@const usable = available(command.id)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li id={`${uid}-${command.id}`} role="option" aria-selected={current?.id === command.id}
          aria-disabled={usable ? undefined : 'true'} onclick={() => { if (usable) onChoose(command.id); }}>
          <span>{t(command.label)}</span>
          {#each bindingsOf(hotkeys, command.id) as chord (chord)}<kbd>{chordText(chord)}</kbd>{/each}
        </li>
      {/each}
    </ul>
    {#if !matches.length}<p role="status">{t('palette.empty')}</p>{/if}
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 10; display: flex; justify-content: center;
    align-items: flex-start; padding: min(12vh, 96px) 12px 12px; background: var(--bg-scrim); }
  .palette { display: flex; flex-direction: column; width: min(480px, 100%); max-height: 100%;
    padding: 14px 8px 8px; background: var(--bg-popover); color: var(--text-primary);
    border: 1px solid var(--border-strong); border-radius: 10px; box-shadow: 0 12px 32px rgb(0 0 0 / 18%); }
  h2 { margin: 0 8px 10px; font-size: 1rem; }
  .field { display: flex; flex: none; gap: 8px; align-items: center; margin: 0 4px 7px; padding: 7px 8px;
    border: 1px solid var(--accent); border-radius: 5px; color: var(--icon-muted); }
  input { flex: 1; min-width: 0; background: transparent; color: var(--text-primary); border: 0; outline: 0; font: inherit; }
  ul { overflow-y: auto; margin: 0; padding: 0; list-style: none; }
  li { display: flex; gap: 4px; align-items: center; padding: 6px 8px; border-radius: 5px; cursor: pointer; }
  li span { flex: 1; }
  li[aria-selected="true"] { background: var(--accent-subtle); color: var(--accent-text); }
  li:not([aria-disabled]):hover { background: var(--bg-hover); }
  li[aria-disabled="true"] { color: var(--text-muted); cursor: default; }
  kbd { font: inherit; font-size: 0.85em; color: var(--text-secondary); }
  p { margin: 8px; color: var(--text-muted); }
</style>
