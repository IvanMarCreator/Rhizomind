<!--
  Вид «Теги» в правой панели (0.2-2.3, С2-3.3 «Список тегов», макет П-2 —
  кадры 66-2781 / 66-3207): переключатель порядка «по имени / по числу
  заметок», теги с числом заметок («#» бледнее имени), щелчок раскрывает
  под тегом его заметки (по имени, папка серым при повторе имени), щелчок
  по заметке открывает её. Данные и порядок — в модели (tags.svelte.ts);
  вид подключает её к событиям ядра, пока он на экране.
-->
<script lang="ts">
  import { untrack } from 'svelte';
  import type { VaultPath } from '../api/index.ts';
  import type { TagOrder } from '../core/layout/layout.ts';
  import { t, type MessageKey } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import TreeIcon from '../tree/TreeIcon.svelte';
  import type { TagsModel } from './tags.svelte.ts';

  let {
    model,
    current,
    onOpen,
  }: {
    model: TagsModel;
    /** Открытая заметка — выделяется в списках заметок тегов. */
    current: VaultPath | null;
    onOpen: (path: VaultPath) => void;
  } = $props();

  const ORDERS: { order: TagOrder; label: MessageKey; icon: 'arrow-down-a-z' | 'arrow-down-wide-narrow' }[] = [
    { order: 'name', label: 'tags.order.name', icon: 'arrow-down-a-z' },
    { order: 'count', label: 'tags.order.count', icon: 'arrow-down-wide-narrow' },
  ];

  let list: HTMLElement | undefined = $state();

  // Подписка на события ядра, пока вид на экране; чтения модели внутри — не зависимости.
  $effect(() => {
    const current = model;
    return untrack(() => current.connect());
  });

  // Тег раскрыли из редактора: прокрутить к нему, когда список готов.
  $effect(() => {
    const key = model.revealed;
    if (key === null || model.status.kind !== 'ready' || !list) return;
    const row = [...list.querySelectorAll<HTMLElement>('[data-key]')].find((el) => el.dataset.key === key);
    row?.scrollIntoView?.({ block: 'nearest' });
    model.revealDone();
  });

  const isCurrent = (path: VaultPath): boolean =>
    current !== null && current.length === path.length && current.every((part, i) => part === path[i]);
</script>

<div class="tags">
  <div class="toolbar">
    <div class="order" role="group" aria-label={t('tags.order')}>
      {#each ORDERS as item (item.order)}
        <button
          type="button"
          class="order-button"
          class:active={model.order === item.order}
          aria-pressed={model.order === item.order}
          aria-label={t(item.label)}
          title={t(item.label)}
          onclick={() => model.setOrder(item.order)}
        >
          <Icon name={item.icon} size={14} />
        </button>
      {/each}
    </div>
  </div>

  {#if model.status.kind === 'building'}
    {@const percent = model.status.percent}
    {@const text = percent === null ? t('tags.indexBuildingUnknown') : t('tags.indexBuilding', { percent })}
    <div class="state">
      <p class="state-text">{text}</p>
      <div
        class="progress"
        role="progressbar"
        aria-label={text}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
      >
        <div class="progress-fill" style:width="{percent ?? 0}%"></div>
      </div>
    </div>
  {:else if model.status.kind === 'loading'}
    <p class="state-text muted">{t('tags.loading')}</p>
  {:else if model.status.kind === 'error'}
    <p class="state-text muted">{t(model.status.message)}</p>
  {:else if model.tags.length === 0}
    <p class="state-text muted">{t('tags.empty')}</p>
  {:else}
    <ul class="list" bind:this={list} aria-label={t('tags.list')}>
      {#each model.tags as summary (summary.key)}
        {@const expanded = model.isExpanded(summary.key)}
        {@const notes = expanded ? model.notesOf(summary.key) : undefined}
        <li data-key={summary.key}>
          <button
            type="button"
            class="tag-row"
            class:expanded
            aria-expanded={expanded}
            aria-label={t('tags.tag', { tag: summary.tag, count: summary.count })}
            onclick={() => model.toggle(summary.key)}
          >
            <span class="chevron" aria-hidden="true"><TreeIcon name="chevron-right" size={14} /></span>
            <span class="hash" aria-hidden="true">#</span><span class="name">{summary.tag}</span>
            <span class="count" aria-hidden="true">{summary.count}</span>
          </button>
          {#if expanded}
            {#if !notes || notes.kind === 'loading'}
              <p class="note-state">{t('tags.notesLoading')}</p>
            {:else if notes.kind === 'error'}
              <p class="note-state">{t(notes.message)}</p>
            {:else if notes.notes.length === 0}
              <p class="note-state">{t('tags.notesEmpty')}</p>
            {:else}
              <ul class="notes" aria-label={t('tags.notes', { tag: summary.tag })}>
                {#each notes.notes as note (note.path.join('/'))}
                  <li>
                    <button
                      type="button"
                      class="note-row"
                      class:current={isCurrent(note.path)}
                      aria-current={isCurrent(note.path) ? 'page' : undefined}
                      title={note.path.join(' / ')}
                      onclick={() => onOpen(note.path)}
                    >
                      <span class="note-name">{note.name}</span>
                      {#if note.folder}<span class="folder">{note.folder}</span>{/if}
                    </button>
                  </li>
                {/each}
              </ul>
              {#if notes.total > notes.notes.length}
                <p class="note-state">{t('tags.notesMore', { count: notes.total - notes.notes.length })}</p>
              {/if}
            {/if}
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .tags {
    display: flex;
    flex-direction: column;
    min-height: 100%;
    font-size: var(--font-size-ui);
  }

  /* Переключатель порядка (tokens.md «0.2»): сегментный, сегменты 24 × 22, иконка 14. */
  .toolbar {
    display: flex;
    flex: none;
    justify-content: flex-end;
    padding: 0 10px 4px;
  }

  .order {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 7px;
    background: var(--bg-hover);
  }

  .order-button {
    display: grid;
    place-items: center;
    width: 24px;
    height: 22px;
    border: 1px solid transparent;
    border-radius: 5px;
    color: var(--icon-muted);
    box-sizing: border-box;
  }

  .order-button:hover:not(.active) {
    color: var(--icon-default);
    cursor: pointer;
  }

  .order-button.active {
    border-color: var(--border-default);
    background: var(--bg-app);
    color: var(--text-primary);
  }

  .order-button:focus-visible,
  .tag-row:focus-visible,
  .note-row:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .list,
  .notes {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .list {
    padding: 0 6px 8px;
  }

  /* Строка тега: высота 26, шеврон 14, «#» `text/faint`, имя 13 `text/secondary`
     (раскрытый — `text/primary` Medium), число 12 `text/muted`. */
  .tag-row,
  .note-row {
    display: flex;
    align-items: center;
    width: 100%;
    min-height: 26px;
    border-radius: 4px;
    color: var(--text-secondary);
    text-align: left;
    white-space: nowrap;
    box-sizing: border-box;
  }

  .tag-row {
    gap: 2px;
    padding: 0 6px 0 2px;
  }

  .tag-row:hover,
  .note-row:hover {
    background: var(--bg-hover);
    cursor: pointer;
  }

  .tag-row.expanded {
    color: var(--text-primary);
    font-weight: 500;
  }

  .chevron {
    display: grid;
    place-items: center;
    flex: none;
    width: 16px;
    height: 20px;
    color: var(--icon-muted);
    transition: transform 0.1s ease;
  }

  .expanded .chevron {
    transform: rotate(90deg);
  }

  .hash {
    flex: none;
    color: var(--text-faint);
    font-weight: 400;
  }

  .name,
  .note-name {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .count {
    flex: none;
    margin-left: auto;
    padding-left: 8px;
    color: var(--text-muted);
    font-size: var(--font-size-caption);
    font-weight: 400;
  }

  /* Заметки тега — отступ 26, открытая — `bg/active`. */
  .note-row {
    gap: 6px;
    padding: 0 6px 0 26px;
  }

  .note-row.current {
    background: var(--bg-active);
    color: var(--text-primary);
  }

  .folder {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text-muted);
    font-size: var(--font-size-caption);
    font-weight: 400;
  }

  .note-state {
    margin: 0;
    padding: 4px 6px 4px 26px;
    color: var(--text-muted);
    font-size: var(--font-size-caption);
  }

  /* «Индекс строится… N %»: текст 13 `text/secondary`, полоса 160 × 4, скругление 2. */
  .state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 24px 14px;
  }

  .state-text {
    margin: 0;
    padding: 8px 14px;
    color: var(--text-secondary);
    text-align: center;
  }

  .state .state-text {
    padding: 0;
  }

  .state-text.muted {
    color: var(--text-muted);
  }

  .progress {
    width: 160px;
    max-width: 100%;
    height: 4px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--bg-active);
  }

  .progress-fill {
    height: 100%;
    background: var(--accent);
  }
</style>
