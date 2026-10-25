<!--
  Быстрый переход (С3.5, макет Э3): окно по Ctrl+O с полем ввода и списком
  заметок. Поиск — в ядре (`switcher_search`, топ-50, нечёткий по имени и
  пути); здесь — подсветка, выбор с клавиатуры и создание заметки, если
  заметки с таким именем нет. Окно рисуется, пока открыто: оболочка решает,
  когда его показать, и получает выбранную заметку через `onOpen`.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { core } from '../api/index.ts';
  import type { SwitcherItem, SwitcherSearchResponse, VaultPath } from '../api/index.ts';
  import { LIMITS } from '../api/limits.ts';
  import { userMessage } from '../i18n/errors.ts';
  import { t } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import { createOffer, newNoteDir, noteFileName } from './create.ts';
  import { highlightItem } from './highlight.ts';
  import { LatestOnly } from './latest.ts';
  import SwitcherIcon from './SwitcherIcon.svelte';

  let {
    currentPath,
    onOpen,
    onClose,
    onNotice,
  }: {
    /** Открытая заметка: для настройки «в папке открытой заметки». */
    currentPath: VaultPath | null;
    onOpen: (path: VaultPath) => void;
    onClose: () => void;
    /** Короткое уведомление внизу окна (предупреждение об имени). */
    onNotice: (text: string) => void;
  } = $props();

  type Option = { kind: 'note'; item: SwitcherItem } | { kind: 'create'; title: string };

  const LISTBOX_ID = 'quick-switcher-list';

  let input: HTMLInputElement | undefined = $state();
  let query = $state('');
  /** Ответ ядра и запрос, на который он дан. */
  let result = $state.raw<{ query: string; items: SwitcherItem[] } | null>(null);
  let searchError = $state<string | null>(null);
  let createError = $state<string | null>(null);
  let creating = $state(false);
  let selected = $state(0);
  /** Окно закрыто (Esc, клик мимо) или убрано оболочкой: незаконченное создание не продолжается. */
  let closed = false;

  /** Строка для ядра: не длиннее предела `max_query_chars` (символов Unicode). */
  function limited(text: string): string {
    const chars = Array.from(text);
    return chars.length > LIMITS.max_query_chars ? chars.slice(0, LIMITS.max_query_chars).join('') : text;
  }

  const searcher = new LatestOnly<SwitcherSearchResponse>(
    (text) => core.switcherSearch({ query: text }),
    (text, response) => {
      result = { query: text, items: response.items };
      searchError = null;
      selected = 0;
    },
    (_, error) => {
      result = null;
      searchError = userMessage(error);
    },
  );

  const sentQuery = $derived(limited(query));
  /** Ответ ядра дан на текущий текст поля: Enter можно выполнять. */
  const ready = $derived(result !== null && result.query === sentQuery);
  const options = $derived.by((): Option[] => {
    if (!result) return [];
    const notes: Option[] = result.items.map((item) => ({ kind: 'note', item }));
    const title = createOffer(result.query, result.items);
    return title === null ? notes : [...notes, { kind: 'create', title }];
  });
  const shownQuery = $derived(result?.query ?? '');

  onMount(() => {
    const previous = document.activeElement;
    input?.focus();
    searcher.request('');
    return () => {
      closed = true;
      searcher.dispose();
      if (previous instanceof HTMLElement) previous.focus();
    };
  });

  function close() {
    if (closed) return;
    closed = true;
    onClose();
  }

  function optionId(index: number): string {
    return `${LISTBOX_ID}-${index}`;
  }

  function onInput(event: Event & { currentTarget: HTMLInputElement }) {
    query = event.currentTarget.value;
    createError = null;
    searcher.request(limited(query));
  }

  function move(step: number) {
    if (options.length === 0) return;
    selected = (selected + step + options.length) % options.length;
    document.getElementById(optionId(selected))?.scrollIntoView?.({ block: 'nearest' });
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.isComposing) return;
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        move(1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        move(-1);
        break;
      case 'Enter':
        event.preventDefault();
        if (ready) void choose(selected);
        break;
      case 'Escape':
        event.preventDefault();
        close();
        break;
      case 'Tab':
        // Окно модальное: фокус остаётся в поле.
        event.preventDefault();
        break;
    }
  }

  async function choose(index: number) {
    const option = options[index];
    if (!option || creating) return;
    if (option.kind === 'note') {
      onOpen(option.item.path);
      close();
      return;
    }
    await create(option.title);
  }

  async function create(title: string) {
    creating = true;
    createError = null;
    try {
      // После каждого ответа ядра: если окно закрыли, дальше не идём. Закрыли
      // до `note_create` — заметка не создаётся; после — она создана, но
      // переход и уведомление не выполняются.
      const { settings } = await core.settingsRead();
      if (closed) return;
      const dir = newNoteDir(settings.new_note_location, currentPath);
      const name = noteFileName(title);
      const check = await core.nameCheck({ dir, name });
      if (closed) return;
      if (check.status === 'rejected') {
        createError = t(`names.${check.problem}`);
        return;
      }
      const { path } = await core.noteCreate({ dir, name });
      if (closed) return;
      onOpen(path);
      close();
      if (check.warning) onNotice(t(`names.warning.${check.warning}`));
    } catch (error) {
      if (!closed) createError = userMessage(error);
    } finally {
      creating = false;
    }
  }

  function onScrimMouseDown(event: MouseEvent) {
    if (event.target === event.currentTarget) close();
  }

  /** Клик по списку и подсказкам не забирает фокус у поля. */
  function keepFocus(event: MouseEvent) {
    event.preventDefault();
  }
</script>

<!-- Затемнение закрывает окно мышью; с клавиатуры окно закрывает Esc в поле. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" data-testid="quick-switcher-scrim" onmousedown={onScrimMouseDown}>
  <div class="switcher" role="dialog" aria-modal="true" aria-label={t('quickSwitcher.label')}>
    <div class="field">
      <span class="search-icon"><SwitcherIcon name="search" /></span>
      <input
        bind:this={input}
        type="text"
        role="combobox"
        aria-label={t('quickSwitcher.label')}
        aria-expanded="true"
        aria-controls={LISTBOX_ID}
        aria-autocomplete="list"
        aria-activedescendant={options.length > 0 ? optionId(selected) : undefined}
        aria-invalid={createError !== null}
        aria-describedby={createError !== null ? 'quick-switcher-error' : undefined}
        placeholder={t('quickSwitcher.placeholder')}
        autocomplete="off"
        spellcheck="false"
        value={query}
        oninput={onInput}
        onkeydown={onKeydown}
      />
      <span class="shortcut">{t('quickSwitcher.shortcut')}</span>
    </div>
    {#if createError !== null}
      <p class="field-error" id="quick-switcher-error" role="alert">{createError}</p>
    {/if}

    <ul class="list" id={LISTBOX_ID} role="listbox" aria-label={t('quickSwitcher.results')} onmousedown={keepFocus}>
      {#each options as option, index (option.kind === 'note' ? option.item.path.join('/') : '\u0000create')}
        <!-- Выбор с клавиатуры — из поля (aria-activedescendant), мышью — кликом. -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          id={optionId(index)}
          role="option"
          class:create={option.kind === 'create'}
          class:selected={index === selected}
          aria-selected={index === selected}
          onmousemove={() => (selected = index)}
          onclick={() => void choose(index)}
        >
          {#if option.kind === 'note'}
            {@const parts = highlightItem(option.item, shownQuery)}
            <span class="icon"><Icon name="file-text" size={16} /></span>
            <span class="name"
              >{#each parts.name as part, i (i)}{#if part.match}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}</span
            >
            {#if parts.folders.length > 0}
              <span class="folders"
                >{#each parts.folders as part, i (i)}{#if part.match}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}</span
              >
            {/if}
          {:else}
            <span class="icon"><SwitcherIcon name="plus" /></span>
            <span class="name">{t('quickSwitcher.create', { name: option.title })}</span>
          {/if}
          {#if index === selected}
            <span class="enter"><SwitcherIcon name="corner-down-left" size={14} /></span>
          {/if}
        </li>
      {/each}
    </ul>
    {#if searchError !== null}
      <p class="message" role="alert">{searchError}</p>
    {:else if ready && options.length === 0}
      <p class="message">{t('quickSwitcher.noNotes')}</p>
    {/if}

    <div class="hints" aria-hidden="true" onmousedown={keepFocus}>
      <span><kbd>{t('quickSwitcher.key.arrows')}</kbd>{t('quickSwitcher.hint.select')}</span>
      <span><kbd>{t('quickSwitcher.key.enter')}</kbd>{t('quickSwitcher.hint.open')}</span>
      <span><kbd>{t('quickSwitcher.key.esc')}</kbd>{t('quickSwitcher.hint.close')}</span>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding: 64px 16px 16px;
    background: var(--bg-scrim);
  }

  .switcher {
    display: flex;
    flex-direction: column;
    width: min(600px, 100%);
    max-height: 100%;
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    background: var(--bg-popover);
    box-shadow: 0 12px 32px rgb(0 0 0 / 18%);
    overflow: hidden;
  }

  .field {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 48px;
    padding: 0 14px;
    border-bottom: 1px solid var(--border-default);
    flex: none;
  }

  .search-icon {
    color: var(--icon-muted);
  }

  input {
    flex: 1;
    min-width: 0;
    height: 100%;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--text-primary);
    caret-color: var(--accent);
    font-size: 14px;
  }

  input::placeholder {
    color: var(--text-faint);
  }

  .shortcut {
    color: var(--text-faint);
    font-size: var(--font-size-caption);
  }

  .field-error {
    margin: 0;
    padding: 8px 14px;
    border-bottom: 1px solid var(--border-default);
    color: var(--status-error);
    font-size: 13px;
  }

  .list {
    margin: 0;
    padding: 6px;
    list-style: none;
    overflow-y: auto;
    min-height: 0;
  }

  .list:empty {
    display: none;
  }

  li {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 40px;
    padding: 0 10px;
    border-radius: 6px;
    color: var(--text-primary);
    cursor: default;
  }

  li.selected {
    background: var(--accent-subtle);
  }

  li.create {
    margin-top: 6px;
    position: relative;
    color: var(--accent-text);
  }

  /* Разделитель над пунктом создания, если над ним есть заметки. */
  li.create:not(:first-child)::before {
    content: '';
    position: absolute;
    top: -4px;
    left: 4px;
    right: 4px;
    border-top: 1px solid var(--border-default);
  }

  .icon {
    display: grid;
    place-items: center;
    color: var(--icon-default);
  }

  li.selected .icon,
  li.create .icon {
    color: var(--accent);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folders {
    flex: 0 1 auto;
    max-width: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-muted);
    font-size: var(--font-size-caption);
  }

  mark {
    background: none;
    color: inherit;
    font-weight: 600;
  }

  .folders mark {
    color: var(--text-secondary);
  }

  .enter {
    color: var(--text-muted);
  }

  .message {
    margin: 0;
    padding: 12px 16px;
    color: var(--text-muted);
  }

  .hints {
    display: flex;
    align-items: center;
    gap: 16px;
    height: 34px;
    padding: 0 14px;
    border-top: 1px solid var(--border-default);
    color: var(--text-muted);
    font-size: var(--font-size-caption);
    flex: none;
  }

  .hints span {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  kbd {
    display: inline-grid;
    place-items: center;
    height: 18px;
    padding: 0 5px;
    border: 1px solid var(--border-strong);
    border-bottom-width: 2px;
    border-radius: 4px;
    font: inherit;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-secondary);
  }
</style>
