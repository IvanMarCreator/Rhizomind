<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { core, CoreCallError, type VaultPath } from '../api/index.ts';
  import { userMessage } from '../i18n/errors.ts';
  import { t } from '../i18n/index.ts';
  import { newNoteDir } from '../quick-switcher/create.ts';
  import FolderPicker from '../settings/FolderPicker.svelte';
  import type { WindowModel } from '../shell/window.svelte.ts';
  import { isHidden } from '../tree/model.ts';

  let { model, onClose }: {
    model: Pick<WindowModel, 'settings' | 'currentPath' | 'updateSettings' | 'open' | 'notify'>;
    onClose: () => void;
  } = $props();

  let folder = $state.raw<VaultPath | null>(untrack(() => model.settings.template_folder ?? null));
  let templates = $state.raw<VaultPath[]>([]);
  let selected = $state.raw<VaultPath | null>(null);
  let filter = $state('');
  let name = $state('');
  let error = $state('');
  let busy = $state(false);
  let loaded = $state(false);
  let dialog: HTMLDivElement;
  let searchInput: HTMLInputElement | undefined = $state();
  let nameInput: HTMLInputElement | undefined = $state();
  const collator = new Intl.Collator('ru', { numeric: true });
  const label = (path: VaultPath) => path.slice(folder?.length ?? 0).join('/');
  const shown = $derived(templates.filter((path) => label(path).toLocaleLowerCase('ru').includes(filter.trim().toLocaleLowerCase('ru'))));

  async function loadTemplates() {
    if (!folder) return;
    error = '';
    loaded = false;
    try {
      const { entries } = await core.treeList();
      const base = folder;
      templates = entries.filter(({ kind, path }) =>
        kind === 'note' && !isHidden(path) && path.length > base.length && base.every((part, i) => path[i] === part),
      ).map(({ path }) => path).sort((a, b) => collator.compare(label(a), label(b)));
    } catch (failure) {
      error = userMessage(failure);
    } finally {
      loaded = true;
    }
  }

  onMount(() => {
    if (folder) {
      searchInput?.focus();
      void loadTemplates();
    }
  });

  async function pickFolder(path: VaultPath) {
    if (busy) return;
    busy = true;
    error = '';
    try {
      await model.updateSettings({ template_folder: path });
      const saved = model.settings.template_folder;
      if (!saved || saved.length !== path.length || !saved.every((part, i) => part === path[i])) {
        error = t('templates.saveFailed');
        return;
      }
      folder = saved;
      await tick();
      searchInput?.focus();
      await loadTemplates();
    } catch (failure) {
      error = userMessage(failure);
    } finally {
      busy = false;
    }
  }

  async function select(path: VaultPath) {
    selected = path;
    name = path.at(-1)?.replace(/\.md$/i, '') ?? '';
    error = '';
    await tick();
    nameInput?.focus();
    nameInput?.select();
  }

  async function create() {
    const title = name.trim();
    if (!selected || !title || busy) return;
    const dir = newNoteDir(model.settings.new_note_location, model.currentPath);
    busy = true;
    error = '';
    try {
      const { path } = await core.noteCreateFromTemplate({
        template: selected, dir, name: /\.md$/i.test(title) ? title : `${title}.md`,
      });
      if (path) model.open(path);
      else model.notify(t('templates.createdUnavailable'));
      onClose();
    } catch (failure) {
      error = failure instanceof CoreCallError && failure.error.code === 'not_implemented'
        ? t('templates.unavailable')
        : userMessage(failure);
    } finally {
      busy = false;
    }
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      if (!busy) onClose();
    } else if (event.key === 'Tab') {
      const items = [...dialog.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled)')];
      const first = items[0];
      const last = items.at(-1);
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault(); last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault(); first?.focus();
      }
    } else if (!selected && folder && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) {
      const rows = [...dialog.querySelectorAll<HTMLButtonElement>('.template-list button')];
      const pos = rows.findIndex((row) => row === document.activeElement);
      if (rows.length) {
        event.preventDefault();
        const next = event.key === 'ArrowDown' ? pos + 1 : pos <= 0 ? rows.length - 1 : pos - 1;
        rows[next % rows.length]?.focus();
      }
    } else if (event.key === 'Enter' && event.target === nameInput) {
      if (event.isComposing) return;
      event.preventDefault();
      if (!event.repeat) void create();
    } else if (event.key === 'Enter' && event.target === searchInput && shown[0]) {
      event.preventDefault();
      void select(shown[0]);
    }
  }
</script>

<div class="backdrop">
  <div class="dialog" role="dialog" aria-modal="true" aria-label={t('templates.title')} tabindex="-1" bind:this={dialog} onkeydown={keydown}>
    <h2>{t('templates.title')}</h2>
    {#if !folder}
      <FolderPicker title={t('templates.folder')} inline onPick={pickFolder} onCancel={() => { if (!busy) onClose(); }} />
    {:else if !selected}
      <label>{t('templates.choose')}
        <input spellcheck="false" aria-label={t('templates.filter')} placeholder={t('templates.filter')} bind:this={searchInput} bind:value={filter} />
      </label>
      <ul class="template-list">
        {#each shown as path (path.join('/'))}
          <li><button type="button" onclick={() => void select(path)}>{label(path)}</button></li>
        {:else}
          {#if loaded && !error}<li>{t('templates.empty')}</li>{/if}
        {/each}
      </ul>
      <div class="actions"><button type="button" onclick={onClose}>{t('templates.cancel')}</button></div>
    {:else}
      <div>
        <p>{label(selected)}</p>
        <label>{t('templates.name')}<input name="note-title" spellcheck="false" bind:this={nameInput} bind:value={name} disabled={busy} /></label>
        <div class="actions">
          <button type="button" disabled={busy} onclick={onClose}>{t('templates.cancel')}</button>
          <button class="primary" type="button" disabled={busy || !name.trim()} onclick={() => void create()}>{t('templates.create')}</button>
        </div>
      </div>
    {/if}
    {#if error}<p role="alert">{error}</p>{/if}
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 30; display: grid; place-items: center; background: var(--bg-overlay, rgb(0 0 0 / 38%)); }
  .dialog { width: min(420px, calc(100vw - 32px)); max-height: 85vh; overflow: auto; box-sizing: border-box; padding: 20px; border: 1px solid var(--border-strong); border-radius: 8px; background: var(--bg-popover); color: var(--text-primary); box-shadow: 0 8px 32px rgb(0 0 0 / 20%); }
  h2 { margin: 0 0 16px; font-size: 1.15rem; }
  label { display: grid; gap: 6px; }
  input, button { padding: 6px 10px; border: 1px solid var(--border-strong); border-radius: 5px; color: inherit; background: var(--bg-app); font: inherit; }
  input:focus-visible, button:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  button { cursor: pointer; }
  .template-list { max-height: 38vh; overflow: auto; list-style: none; margin: 8px 0; padding: 0; }
  .template-list button { width: 100%; text-align: left; border-color: transparent; background: transparent; overflow-wrap: anywhere; }
  .template-list button:hover, .template-list button:focus-visible { background: var(--accent-subtle); }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; }
  .primary { color: var(--accent-text); background: var(--accent-subtle); }
  button:disabled { opacity: .5; cursor: default; }
  p { margin: 8px 0; overflow-wrap: anywhere; }
</style>
