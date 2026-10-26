<!--
  Раздел «Файлы и ссылки» (Э12.3): где создавать новые заметки,
  подтверждение удаления, лимит размера для редактирования. Обновление
  ссылок при переименовании — с 0.2.
-->
<script lang="ts">
  import { untrack } from 'svelte';
  import type { NewNoteLocation, Settings, VaultPath } from '../../api/index.ts';
  import { LIMITS } from '../../api/limits.ts';
  import { t } from '../../i18n/index.ts';
  import Icon from '../../shell/Icon.svelte';
  import FolderPicker from '../FolderPicker.svelte';
  import NumberField from '../NumberField.svelte';
  import Row from '../Row.svelte';
  import Segmented from '../Segmented.svelte';
  import Toggle from '../Toggle.svelte';

  let { settings, onChange }: { settings: Settings; onChange: (patch: Partial<Settings>) => void } = $props();

  const MIB = 1024 * 1024;
  const [minBytes, maxBytes] = LIMITS.edit_size_range;
  let picking = $state(false);
  /** Папка «В указанной папке», пока выбран другой вариант: возвращается при повторном выборе. */
  let lastFolder = $state.raw<VaultPath>(
    untrack(() => (settings.new_note_location.kind === 'folder' ? settings.new_note_location.path : [])),
  );

  const location = $derived(settings.new_note_location);

  function setKind(kind: NewNoteLocation['kind']) {
    const next: NewNoteLocation = kind === 'folder' ? { kind, path: lastFolder } : { kind };
    onChange({ new_note_location: next });
  }

  function pick(path: VaultPath) {
    picking = false;
    lastFolder = path;
    onChange({ new_note_location: { kind: 'folder', path } });
  }
</script>

<div class="block">
  <Row title={t('settings.newNote.title')} hint={t('settings.newNote.hint')}>
    <span class="select-wrap">
      <select
        class="select"
        aria-label={t('settings.newNote.title')}
        value={location.kind}
        onchange={(event) => setKind(event.currentTarget.value as NewNoteLocation['kind'])}
      >
        <option value="vault_root">{t('settings.newNote.root')}</option>
        <option value="current_folder">{t('settings.newNote.current')}</option>
        <option value="folder">{t('settings.newNote.folder')}</option>
      </select>
      <span class="chevron" aria-hidden="true"><Icon name="chevron-down" size={14} /></span>
    </span>
  </Row>
  {#if location.kind === 'folder'}
    <div class="folder-row">
      <span class="folder" data-testid="new-note-folder">
        <Icon name="folder" size={14} />
        <span>{location.path.length === 0 ? t('settings.newNote.rootFolder') : location.path.join(' / ')}</span>
      </span>
      <div class="anchor">
        <button type="button" class="button" onclick={() => (picking = !picking)}>{t('settings.newNote.pick')}</button>
        {#if picking}
          <FolderPicker onPick={pick} onCancel={() => (picking = false)} />
        {/if}
      </div>
    </div>
  {/if}
</div>
<Row
  id="settings-rename-links"
  title={t('settings.renameLinks.title')}
  hint={t('settings.renameLinks.hint')}
  badge={t('settings.badge.release02')}
  disabled
  singleLine
>
  <Segmented
    compact
    labelledby="settings-rename-links"
    value={null}
    disabled
    options={[
      { value: 'ask', label: t('settings.renameLinks.ask') },
      { value: 'update', label: t('settings.renameLinks.update') },
    ]}
  />
</Row>
<Row title={t('settings.confirmDelete.title')} hint={t('settings.confirmDelete.hint')}>
  <Toggle
    checked={settings.confirm_delete}
    label={t('settings.confirmDelete.title')}
    onChange={(confirm_delete) => onChange({ confirm_delete })}
  />
</Row>
<Row title={t('settings.maxEdit.title')} hint={t('settings.maxEdit.hint')}>
  <NumberField
    value={Math.round(settings.max_edit_size_bytes / MIB)}
    min={minBytes / MIB}
    max={maxBytes / MIB}
    label={t('settings.maxEdit.title')}
    unit={t('settings.maxEdit.unit')}
    onChange={(mib) => onChange({ max_edit_size_bytes: mib * MIB })}
  />
</Row>

<style>
  .block {
    border-bottom: 1px solid var(--border-default);
  }

  .block :global(.row) {
    border-bottom: none;
  }

  .select-wrap {
    position: relative;
    display: inline-flex;
  }

  .chevron {
    position: absolute;
    top: 50%;
    right: 8px;
    color: var(--icon-default);
    transform: translateY(-50%);
    pointer-events: none;
  }

  .select {
    min-width: 190px;
    height: 28px;
    padding: 0 28px 0 8px;
    appearance: none;
    cursor: pointer;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    background: var(--bg-app);
    color: var(--text-primary);
    font: inherit;
  }

  .select:focus {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  .folder-row {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 0 0 14px;
  }

  .folder {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 190px;
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--border-default);
    border-radius: 5px;
    color: var(--text-primary);
    box-sizing: border-box;
  }

  .folder span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder :global(.icon) {
    flex: none;
    color: var(--icon-default);
  }

  .anchor {
    position: relative;
  }

  .button {
    height: 28px;
    padding: 0 10px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    background: var(--bg-app);
    color: var(--text-primary);
  }

  .button:hover {
    background: var(--bg-hover);
  }
</style>
