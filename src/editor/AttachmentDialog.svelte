<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { core, type VaultPath } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import { userMessage } from '../i18n/errors.ts';
  import { finishAttachment, type AttachmentAnchor } from './attachment.ts';

  let { anchor, initialFile, notify, onClose }: { anchor: AttachmentAnchor; initialFile?: VaultPath | null; notify: (message: string) => void; onClose: () => void } = $props();
  let files = $state.raw<VaultPath[]>([]);
  let selected = $state.raw<VaultPath | null>(null);
  let filter = $state('');
  let name = $state('');
  let error = $state('');
  let busy = $state(false);
  let loaded = $state(false);
  let input: HTMLInputElement | undefined = $state();
  let title: HTMLInputElement | undefined = $state();
  let dialog: HTMLDivElement;
  const external = $derived(/^(?:[a-zA-Z]:|[\\/]|file:|\.\.(?:[\\/]|$))/.test(filter.trim()));
  const shown = $derived(files.filter((p) => p.join('/').toLocaleLowerCase('ru').includes(filter.toLocaleLowerCase('ru'))));
  const valid = $derived(name.length > 0 && !name.startsWith('.') && !name.startsWith(' ') && !/[#[\]|\\/]/.test(name));

  onMount(() => {
    if (initialFile) { void choose(initialFile); return; }
    input?.focus();
    void core.treeList().then(({ entries }) => {
      files = entries.filter(({ kind }) => kind !== 'folder').map(({ path }) => path)
        .sort((a, b) => a.join('/').localeCompare(b.join('/'), 'ru', { numeric: true }));
    }).catch((failure: unknown) => { error = userMessage(failure); }).finally(() => { loaded = true; });
  });

  async function choose(path: VaultPath) {
    selected = path;
    name = path.at(-1) ?? '';
    error = '';
    await tick();
    title?.focus();
    title?.select();
  }

  async function save(clipboard = false) {
    if (busy || (!clipboard && !valid)) return;
    const source = clipboard ? { kind: 'clipboard' as const } : selected ? { kind: 'file' as const, path: selected } : null;
    if (!source) return;
    busy = true;
    error = '';
    try {
      const result = await core.attachmentInsert({
        note: anchor.note, source,
        name: clipboard ? t('attachments.screenshot') : name,
      });
      if (result.outcome === 'choose_name') { await choose(result.path); return; }
      if (result.outcome === 'text') anchor.pasteText(result.text);
      else finishAttachment(result, anchor.insert, notify);
      onClose();
    } catch (failure) { error = userMessage(failure); }
    finally { busy = false; }
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape') { event.preventDefault(); if (!busy) onClose(); }
    if (event.key === 'Enter' && !event.isComposing && !event.repeat) {
      if (event.target === title) { event.preventDefault(); void save(); }
      if (event.target === input && shown[0] && !external) { event.preventDefault(); void choose(shown[0]); }
    }
    if (event.key === 'Tab') {
      const controls = [...dialog.querySelectorAll<HTMLElement>('input:not(:disabled),button:not(:disabled)')];
      const first = controls[0], last = controls.at(-1);
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    }
  }
</script>

<div class="backdrop">
  <div bind:this={dialog} role="dialog" aria-modal="true" aria-label={t('attachments.insert')} tabindex="-1" onkeydown={keydown}>
    <h2>{t('attachments.insert')}</h2>
    {#if selected}
      <p>{selected.join('/')}</p>
      <label>{t('attachments.name')}<input bind:this={title} bind:value={name} spellcheck="false" disabled={busy} /></label>
      {#if !valid}<p role="status">{t('attachments.rename')}</p>{/if}
    {:else}
      <label>{t('attachments.choose')}<input bind:this={input} bind:value={filter} spellcheck="false" /></label>
      {#if external}<p role="status">{t('attachments.external')}</p>
      {:else}
        <ul>{#each shown as file (file.join('/'))}<li><button type="button" onclick={() => void choose(file)}>{file.join('/')}</button></li>{/each}</ul>
        {#if loaded && !shown.length}<p>{t('search.empty')}</p>{/if}
      {/if}
    {/if}
    {#if error}<p role="alert">{error}</p>{/if}
    <footer>
      <button type="button" disabled={busy} onclick={onClose}>{t('templates.cancel')}</button>
      {#if selected}
        <button type="button" disabled={busy || !valid} onclick={() => void save()}>{t('attachments.insert')}</button>
      {:else}
        <button type="button" disabled={busy} onclick={() => void save(true)}>{t('attachments.fromClipboard')}</button>
      {/if}
    </footer>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 30; display: grid; place-items: center; background: var(--bg-overlay, rgb(0 0 0 / 38%)); }
  [role=dialog] { box-sizing: border-box; width: min(440px, calc(100vw - 32px)); max-height: 85vh; overflow: auto; padding: 20px; color: var(--text-primary); background: var(--bg-popover); border: 1px solid var(--border-strong); border-radius: 8px; }
  h2 { margin: 0 0 16px; font-size: 1.15rem; }
  label { display: grid; gap: 6px; }
  input, button { font: inherit; color: inherit; border: 1px solid var(--border-default); border-radius: 4px; background: var(--bg-app); padding: 6px 9px; }
  button { cursor: pointer; } button:disabled { opacity: .5; cursor: default; }
  ul { max-height: 38vh; overflow: auto; list-style: none; margin: 8px 0; padding: 0; }
  li button { width: 100%; text-align: left; border: 0; overflow-wrap: anywhere; }
  li button:hover, li button:focus-visible { background: var(--accent-subtle); }
  footer { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; }
  p { overflow-wrap: anywhere; }
</style>
