<script lang="ts">
  import type { Settings, VaultPath } from '../api/index.ts';
  import { LIMITS } from '../api/limits.ts';
  import { t } from '../i18n/index.ts';
  import FolderPicker from './FolderPicker.svelte';
  import Row from './Row.svelte';

  let { settings, onChange }: { settings: Settings; onChange: (patch: Partial<Settings>) => void } = $props();
  let extension = $state('');
  let choosing = $state<string | null>(null);
  const key = $derived(extension.trim().replace(/^\./, '').toLowerCase());
  const valid = $derived(key.length <= LIMITS.max_component_utf16 && /^[\p{L}\p{N}_-]+$/u.test(key));
  const rules = $derived(Object.entries(settings.attachment_folders ?? {}).sort(([a], [b]) => a.localeCompare(b)));

  function choose(path: VaultPath) {
    if (!choosing) return;
    onChange({ attachment_folders: { ...settings.attachment_folders, [choosing]: path } });
    choosing = null;
    extension = '';
  }

  function remove(ext: string) {
    const remaining = rules.filter(([extension]) => extension !== ext);
    onChange({ attachment_folders: Object.fromEntries(remaining) });
  }
</script>

<Row title={t('attachments.folderRules')} hint={t('attachments.defaultFolder')}>
  <div class="rules">
    {#each rules as [ext, folder] (ext)}
      <div class="rule">
        <span>.{ext} — {folder.join(' / ') || t('settings.newNote.rootFolder')}</span>
        <button type="button" aria-label={t('attachments.removeRule', { ext })} onclick={() => remove(ext)}>{t('attachments.remove')}</button>
      </div>
    {/each}
    <div class="rule">
      <input spellcheck="false" maxlength={LIMITS.max_component_utf16 + 1} aria-label={t('attachments.extension')}
        placeholder={t('attachments.extension')} bind:value={extension} />
      <button type="button" disabled={!valid} onclick={() => (choosing = key)}>{t('attachments.chooseFolder')}</button>
    </div>
    {#if choosing}
      <FolderPicker title={t('attachments.folder')} onPick={choose} onCancel={() => (choosing = null)} />
    {/if}
  </div>
</Row>

<style>
  .rules { position: relative; display: grid; gap: 8px; }
  .rule { display: flex; align-items: center; justify-content: flex-end; flex-wrap: wrap; gap: 8px; }
  input, button {
    border: 1px solid var(--border-default);
    border-radius: 4px;
    background: var(--bg-popover);
    color: var(--text-primary);
    padding: 4px 8px;
    font: inherit;
  }
  input { width: 10em; min-width: 0; }
  button { cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }
</style>
