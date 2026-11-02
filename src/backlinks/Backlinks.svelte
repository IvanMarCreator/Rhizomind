<!--
  Вид «Обратные ссылки» правой панели (0.2-2.2, С2-3.2, макет П-А — кадры
  66-2 и 65-2, состояния П-3 66-3631). Группы по заметкам в порядке ядра:
  шеврон, имя (и серым — папка, если имя повторяется), справа — число ссылок;
  под ним строки с выделенной ссылкой и направляющей слева. Строка — только
  текст из файла (никакого HTML из заметки). Щелчок по строке — заметка с
  курсором на ссылке, по имени — заметка сверху; Ctrl+щелчок — так же
  (вкладок в 0.2 нет). Число в заголовке панели ставит вид (`panel.setCount`).
-->
<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { onCoreEvent, type BacklinkNote, type VaultPath } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import type { RightPanelModel } from '../sidebar-right/panel.svelte.ts';
  import {
    BacklinksView,
    folderOf,
    positionOf,
    repeatedNames,
    splitLine,
    type BacklinksSource,
    type LinkPosition,
  } from './view.svelte.ts';
  import { nameKey } from '../core/names/index.ts';

  let {
    path,
    panel,
    onOpen,
    source,
  }: {
    /** Открытая заметка; `null` — не открыта. */
    path: VaultPath | null;
    panel: RightPanelModel;
    /** Открыть заметку: курсор на ссылку `at` или, при `null`, в начало. */
    onOpen: (path: VaultPath, at: LinkPosition | null) => void;
    /** Источник данных (по умолчанию — команда ядра `backlinks_list`). */
    source?: BacklinksSource;
  } = $props();

  const view = new BacklinksView(untrack(() => source));

  /** Свёрнутые группы (пути через `/`); не запоминаются. */
  const collapsed = new SvelteSet<string>();

  $effect(() => view.show(path));
  $effect(() => panel.setCount('backlinks', view.count));

  /** Повторяется ли имя (ключ `nameKey`) в списке: тогда рядом — папка. */
  const repeated = $derived(repeatedNames(view.state.kind === 'ready' ? view.state.data.notes : []));

  function keyOf(note: BacklinkNote): string {
    return note.from.join('/');
  }

  function toggle(note: BacklinkNote): void {
    const key = keyOf(note);
    if (!collapsed.delete(key)) collapsed.add(key);
  }

  function folderLabel(note: BacklinkNote): string | null {
    if (!repeated(nameKey(note.name))) return null;
    return folderOf(note.from) || t('backlinks.vaultRoot');
  }

  onMount(() => {
    let disposed = false;
    const stops: (() => void)[] = [];
    const keep = (pending: Promise<() => void>) =>
      pending
        .then((stop) => {
          if (disposed) stop();
          else stops.push(stop);
        })
        .catch(() => undefined);
    keep(onCoreEvent('index:changed', () => view.refresh()));
    keep(onCoreEvent('index:status', (status) => view.indexStatus(status)));
    return () => {
      disposed = true;
      for (const stop of stops) stop();
    };
  });
</script>

<div class="backlinks">
  {#if view.state.kind === 'none' || (view.state.kind === 'ready' && view.state.data.notes.length === 0)}
    <div class="state">
      <span class="state-icon"><Icon name="link" size={18} /></span>
      <p class="state-text">{t('backlinks.empty')}</p>
    </div>
  {:else if view.state.kind === 'building'}
    {@const percent = view.state.percent}
    <div class="state">
      <span class="state-icon spin"><Icon name="loader-circle" size={18} /></span>
      <p class="state-text">
        {percent === null ? t('backlinks.building') : t('backlinks.buildingPercent', { percent })}
      </p>
      <div
        class="bar"
        role="progressbar"
        aria-label={t('backlinks.building')}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
      >
        <div class="bar-fill" style:width="{percent ?? 0}%"></div>
      </div>
    </div>
  {:else if view.state.kind === 'failed'}
    <p class="failed" role="alert">{t('backlinks.failed', { message: view.state.message })}</p>
  {:else if view.state.kind === 'ready'}
    {@const data = view.state.data}
    <ul class="groups" aria-label={t('backlinks.list')}>
      {#each data.notes as note (keyOf(note))}
        {@const open = !collapsed.has(keyOf(note))}
        {@const folder = folderLabel(note)}
        <li class="group">
          <div class="head">
            <button
              type="button"
              class="chevron"
              class:closed={!open}
              aria-expanded={open}
              aria-label={open ? t('backlinks.collapse') : t('backlinks.expand')}
              onclick={() => toggle(note)}
            >
              <Icon name="chevron-down" size={14} />
            </button>
            <button
              type="button"
              class="note"
              aria-label={folder === null
                ? t('backlinks.open', { name: note.name })
                : t('backlinks.openIn', { name: note.name, folder })}
              onclick={() => onOpen(note.from, null)}
            >
              <span class="name">{note.name}</span>
              {#if folder !== null}<span class="folder">{folder}</span>{/if}
            </button>
            <span class="links-total">{note.links_total}</span>
          </div>
          {#if open}
            <ul class="lines">
              {#each note.links as line, i (i)}
                {@const part = splitLine(line)}
                <li>
                  <button type="button" class="line" onclick={() => onOpen(note.from, positionOf(line))}
                    >{part.before}<mark class="link">{part.link}</mark>{part.after}</button
                  >
                </li>
              {/each}
            </ul>
          {/if}
        </li>
      {/each}
    </ul>
    {#if data.truncated}
      <p class="truncated">{t('backlinks.truncated')}</p>
    {/if}
  {/if}
</div>

<style>
  /* tokens.md, «0.2», «Группа обратных ссылок» (П-А). */
  .backlinks {
    height: 100%;
    box-sizing: border-box;
  }

  .groups,
  .lines {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .groups {
    padding: 2px 8px 12px;
  }

  .group + .group {
    margin-top: 6px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 2px;
    min-height: 26px;
  }

  .chevron {
    display: grid;
    flex: none;
    place-items: center;
    width: 18px;
    height: 22px;
    border-radius: 4px;
    color: var(--icon-muted);
  }

  .chevron.closed :global(.icon) {
    transform: rotate(-90deg);
  }

  .note {
    display: flex;
    flex: 1;
    align-items: baseline;
    gap: 6px;
    min-width: 0;
    padding: 3px 4px;
    border-radius: 4px;
    text-align: left;
  }

  .name {
    overflow: hidden;
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--text-muted);
    font-size: 12px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .links-total {
    flex: none;
    padding: 0 4px 0 6px;
    color: var(--text-muted);
    font-size: 12px;
  }

  /* Направляющая 1 px слева, отступ 10 + 12. */
  .lines {
    margin: 2px 0 0 10px;
    padding-left: 0;
    border-left: 1px solid var(--border-default);
  }

  .line {
    display: block;
    width: 100%;
    padding: 4px 6px 4px 11px;
    border-radius: 0 4px 4px 0;
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 18px;
    text-align: left;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .link {
    background: none;
    color: var(--accent-text);
    font-weight: 500;
  }

  .chevron:hover,
  .note:hover,
  .line:hover {
    background: var(--bg-hover);
  }

  .chevron:focus-visible,
  .note:focus-visible,
  .line:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  /* П-3: «Индекс строится… N %» и «Нет обратных ссылок» — по центру вида. */
  .state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 100%;
    min-height: 120px;
    padding: 0 16px 40px;
    box-sizing: border-box;
  }

  .state-icon {
    color: var(--icon-muted);
  }

  .spin :global(.icon) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spin :global(.icon) {
      animation: none;
    }
  }

  .state-text {
    margin: 0;
    color: var(--text-secondary);
    font-size: 13px;
    text-align: center;
  }

  .bar {
    width: 160px;
    height: 4px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--bg-active);
  }

  .bar-fill {
    height: 100%;
    border-radius: 2px;
    background: var(--accent);
  }

  .failed,
  .truncated {
    margin: 8px 14px;
    color: var(--text-muted);
    font-size: 12px;
  }
</style>
