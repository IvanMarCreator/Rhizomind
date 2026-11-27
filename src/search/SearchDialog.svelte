<script lang="ts">
  import { onMount } from 'svelte';
  import { core, onCoreEvent, type IndexReply, type SearchHit, type SearchResults, type VaultPath } from '../api/index.ts';
  import { LIMITS } from '../api/limits.ts';
  import type { LinkPosition } from '../editor/wikilinks/host.ts';
  import { t } from '../i18n/index.ts';

  let { current, onOpen, onClose }: {
    current: VaultPath | null;
    onOpen: (path: VaultPath, at: LinkPosition | null) => void;
    onClose: () => void;
  } = $props();

  let field: HTMLInputElement;
  let dialog: HTMLDivElement;
  let text = $state('');
  let folder = $state('');
  let tag = $state('');
  let mentions = $state(false);
  let revision = $state(0);
  let reply = $state.raw<IndexReply<SearchResults> | null>(null);
  let failed = $state(false);
  const active = $derived(Boolean(text.trim() || folder.trim() || tag.trim() || (mentions && current)));

  function search(_revision: number) {
    const request = {
      text,
      folder: folder.trim() ? folder.trim().split(/[/\\]/u) : null,
      tag: tag.trim().replace(/^#/u, '') || null,
      mentions_of: mentions ? current : null,
    };
    let valid = true;
    reply = null;
    failed = false;
    void core.vaultSearch(request).then((result) => {
      if (valid) reply = result;
    }, () => { if (valid) failed = true; });
    return () => { valid = false; };
  }

  $effect(() => search(revision));

  onMount(() => {
    field.focus();
    const listeners = [
      onCoreEvent('index:changed', () => { revision++; }),
      onCoreEvent('index:status', (status) => {
        if (status.state === 'ready') revision++;
        else reply = { status: 'index_building', done: status.done, total: status.total };
      }),
    ];
    return () => {
      for (const ready of listeners) void ready.then((unlisten) => unlisten()).catch(() => undefined);
    };
  });

  function openHit(hit: SearchHit): void {
    onOpen(hit.path, hit.location ? { line: hit.location.line, column: hit.location.column_utf16 } : null);
  }

  function keydown(event: KeyboardEvent): void {
    if (event.isComposing) return;
    const buttons = [...dialog.querySelectorAll<HTMLButtonElement>('.result')];
    const currentButton = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === 'Escape') {
      event.preventDefault();
      onClose();
    } else if (event.key === 'Enter' && event.target === field && reply?.status === 'ready' && reply.items[0]) {
      event.preventDefault();
      openHit(reply.items[0]);
    } else if (['ArrowDown', 'ArrowUp'].includes(event.key) && (event.target === field || currentButton >= 0)) {
      event.preventDefault();
      if (event.key === 'ArrowUp' && currentButton <= 0) field.focus();
      else buttons[Math.min(buttons.length - 1, currentButton + (event.key === 'ArrowDown' ? 1 : -1))]?.focus();
    } else if (event.key === 'Tab') {
      const controls = [...dialog.querySelectorAll<HTMLElement>('input:not(:disabled), button:not(:disabled)')];
      const first = controls[0], last = controls.at(-1);
      const next = event.shiftKey && document.activeElement === first ? last
        : !event.shiftKey && document.activeElement === last ? first : null;
      if (next) { event.preventDefault(); next.focus(); }
    }
  }
</script>

<div class="scrim">
  <div class="vault-search" role="dialog" aria-modal="true" aria-label={t('search.title')} tabindex="-1" bind:this={dialog} onkeydown={keydown}>
    <header><h2>{t('search.title')}</h2><button class="close" onclick={onClose}>{t('settings.close')}</button></header>
    <input class="query" type="search" bind:this={field} bind:value={text} aria-label={t('search.words')}
      placeholder={t('search.words')} maxlength={LIMITS.max_query_chars} autocomplete="off" spellcheck="false"/>
    <div class="filters">
      <label>{t('search.folder')} <input bind:value={folder} placeholder={t('search.allFolders')} autocomplete="off" spellcheck="false"/></label>
      <label>{t('search.tag')} <input bind:value={tag} placeholder={t('search.allTags')} autocomplete="off" spellcheck="false" maxlength={LIMITS.max_tag_key_chars}/></label>
    </div>
    <label class="mentions"><input type="checkbox" spellcheck="false" bind:checked={mentions} disabled={!current}/>{t('search.mentions')}{#if current}<span>{current.at(-1)}</span>{/if}</label>
    {#if failed}
      <p role="alert">{t('search.failed')}</p>
    {:else if reply?.status === 'index_building'}
      <p role="status">{reply.total ? t('backlinks.buildingPercent', { percent: Math.floor(100 * reply.done / reply.total) }) : t('backlinks.building')}</p>
    {:else if reply?.status === 'ready' && active}
      {#if reply.items.length}
        <ul aria-label={t('search.results')}>
          {#each reply.items as hit (hit.path.join('/'))}
            <li><button class="result" onclick={() => openHit(hit)}>
              <strong>{hit.path.join(' / ')}</strong>
              {#if hit.location}<span>{hit.location.line} · {hit.location.snippet}</span>{/if}
            </button></li>
          {/each}
        </ul>
        {#if reply.truncated}<p role="status">{t('search.truncated')}</p>{/if}
      {:else}<p role="status">{t('search.empty')}</p>{/if}
    {/if}
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 10; display: flex; align-items: flex-start; justify-content: center;
    padding: min(9vh, 64px) 12px 12px; background: var(--bg-scrim); }
  .vault-search { display: flex; flex-direction: column; gap: 12px; width: min(560px, 100%); max-height: 100%;
    box-sizing: border-box; padding: 16px; border: 1px solid var(--border-strong); border-radius: 10px;
    background: var(--bg-popover); color: var(--text-primary); box-shadow: 0 12px 32px rgb(0 0 0 / 18%); }
  header, .filters, .mentions { display: flex; align-items: center; gap: 12px; }
  header { justify-content: space-between; }
  h2, p, ul { margin: 0; }
  h2 { font-size: 1.05rem; }
  input, button { font: inherit; color: inherit; }
  input:not([type="checkbox"]) { box-sizing: border-box; width: 100%; padding: 7px 9px; border: 1px solid var(--border-strong);
    border-radius: 5px; background: var(--bg-popover); }
  input:focus-visible, button:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .filters label { flex: 1; min-width: 0; color: var(--text-secondary); font-size: .9rem; }
  .filters input { margin-top: 3px; }
  .mentions { gap: 6px; font-size: .9rem; }
  .mentions span { color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; }
  ul { overflow-y: auto; list-style: none; padding: 2px; }
  button { cursor: pointer; background: transparent; border: 0; border-radius: 4px; }
  .close, p { color: var(--text-secondary); }
  .result { display: flex; flex-direction: column; align-items: flex-start; width: 100%; padding: 7px; text-align: left; overflow-wrap: anywhere; }
  .result:hover, .result:focus { background: var(--bg-hover); }
  .result span { color: var(--text-secondary); }
</style>
