<!--
  Окно «Настройки хранилища <имя>» (С3.1, макеты Э12; решение 72): разделы
  слева, содержимое справа. Изменение применяется сразу и записывается в
  `.rhizomind/settings.json` хранилища (WindowModel.updateSettings). Esc —
  закрыть (во время записи сочетания Esc отменяет запись).
-->
<script lang="ts" module>
  export type SettingsSection = 'general' | 'editor' | 'files' | 'appearance' | 'hotkeys' | 'about';
</script>

<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { CurrentVault, Settings } from '../api/index.ts';
  import { t, type MessageKey } from '../i18n/index.ts';
  import Icon, { type IconName } from '../shell/Icon.svelte';
  import { vaultLetter } from '../vaults/letter.ts';
  import AboutSection from './sections/AboutSection.svelte';
  import AppearanceSection from './sections/AppearanceSection.svelte';
  import EditorSection from './sections/EditorSection.svelte';
  import FilesSection from './sections/FilesSection.svelte';
  import GeneralSection from './sections/GeneralSection.svelte';
  import HotkeysSection from './sections/HotkeysSection.svelte';

  let {
    vault,
    settings,
    section = 'general',
    onChange,
    onClose,
  }: {
    vault: CurrentVault;
    settings: Settings;
    /** Раздел при открытии (из «Открыть настройки» у большого файла — «Файлы и ссылки»). */
    section?: SettingsSection;
    onChange: (patch: Partial<Settings>) => void;
    onClose: () => void;
  } = $props();

  const SECTIONS: readonly { id: SettingsSection; label: MessageKey; icon: IconName }[] = [
    { id: 'general', label: 'settings.section.general', icon: 'settings' },
    { id: 'editor', label: 'settings.section.editor', icon: 'pencil' },
    { id: 'files', label: 'settings.section.files', icon: 'folder' },
    { id: 'appearance', label: 'settings.section.appearance', icon: 'sun' },
    { id: 'hotkeys', label: 'settings.section.hotkeys', icon: 'keyboard' },
    { id: 'about', label: 'settings.section.about', icon: 'info' },
  ];

  // Раздел при открытии; дальше переключает пользователь.
  let current = $state<SettingsSection>(untrack(() => section));
  let dialog: HTMLDivElement | undefined = $state();
  const currentLabel = $derived(SECTIONS.find((s) => s.id === current)?.label ?? 'settings.section.general');

  onMount(() => {
    const previous = document.activeElement;
    dialog?.querySelector<HTMLElement>('[aria-current="page"]')?.focus();
    return () => {
      if (previous instanceof HTMLElement) previous.focus();
    };
  });

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    } else if (event.key === 'Tab' && dialog) {
      // Окно модальное: фокус ходит по его элементам по кругу.
      const items = [
        ...dialog.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled)'),
      ];
      const first = items[0];
      const last = items.at(-1);
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    }
  }
</script>

<div class="scrim">
  <div
    bind:this={dialog}
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label={t('settings.title', { name: vault.name })}
    tabindex="-1"
    {onkeydown}
  >
    <aside class="side">
      <div class="side-head">
        <div class="caption">{t('settings.label')}</div>
        <div class="vault">
          <span class="letter" aria-hidden="true">{vaultLetter(vault.name)}</span>
          <span class="vault-name">{vault.name}</span>
        </div>
      </div>
      <nav aria-label={t('settings.nav.label')}>
        {#each SECTIONS as item (item.id)}
          <button
            type="button"
            class="nav-item"
            aria-current={item.id === current ? 'page' : undefined}
            onclick={() => (current = item.id)}
          >
            <Icon name={item.icon} size={15} />{t(item.label)}
          </button>
        {/each}
      </nav>
      <div class="stored">
        {t('settings.storedIn')}<br /><code>{t('settings.storedFile')}</code>
      </div>
    </aside>
    <section class="main" aria-labelledby="settings-section-title">
      <header class="head">
        <div>
          <div class="breadcrumb">{t('settings.title', { name: vault.name })}</div>
          <h2 id="settings-section-title">{t(currentLabel)}</h2>
        </div>
        <button type="button" class="close" aria-label={t('settings.close')} title={t('settings.close')} onclick={onClose}>
          <Icon name="x" size={16} />
        </button>
      </header>
      <div class="content">
        {#if current === 'general'}
          <GeneralSection {vault} />
        {:else if current === 'editor'}
          <EditorSection {settings} {onChange} />
        {:else if current === 'files'}
          <FilesSection {settings} {onChange} />
        {:else if current === 'appearance'}
          <AppearanceSection {settings} {onChange} />
        {:else if current === 'hotkeys'}
          <HotkeysSection hotkeys={settings.hotkeys} onChange={(hotkeys) => onChange({ hotkeys })} />
        {:else}
          <AboutSection />
        {/if}
      </div>
    </section>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 30;
    display: grid;
    place-items: center;
    background: var(--bg-scrim);
  }

  .dialog {
    display: flex;
    width: 920px;
    max-width: calc(100vw - 48px);
    height: 620px;
    max-height: calc(100vh - 48px);
    overflow: hidden;
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    background: var(--bg-popover);
    box-shadow: 0 16px 48px rgb(0 0 0 / 22%);
  }

  .dialog:focus {
    outline: none;
  }

  .side {
    display: flex;
    flex: none;
    flex-direction: column;
    width: 220px;
    padding: 18px 10px 14px;
    border-right: 1px solid var(--border-default);
    background: var(--bg-sidebar);
    box-sizing: border-box;
  }

  .side-head {
    padding: 0 8px 12px;
  }

  .caption {
    color: var(--text-muted);
    font-size: var(--font-size-heading);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .vault {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 6px;
  }

  .letter {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 4px;
    background: var(--accent-subtle);
    color: var(--accent-text);
    font-size: 11px;
    font-weight: 600;
  }

  .vault-name {
    overflow: hidden;
    color: var(--text-primary);
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  nav {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 30px;
    padding: 0 8px;
    border-radius: 5px;
    color: var(--text-secondary);
    text-align: left;
  }

  .nav-item :global(.icon) {
    color: var(--icon-default);
  }

  .nav-item:hover {
    background: var(--bg-hover);
  }

  .nav-item[aria-current='page'] {
    background: var(--bg-active);
    color: var(--text-primary);
  }

  .nav-item:focus-visible,
  .close:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  .stored {
    padding: 0 8px;
    color: var(--text-muted);
    font-size: 11px;
    line-height: 16px;
  }

  code {
    font-family: var(--font-mono);
  }

  .main {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .head {
    display: flex;
    justify-content: space-between;
    padding: 18px 24px 6px;
  }

  .breadcrumb {
    color: var(--text-muted);
    font-size: var(--font-size-caption);
  }

  h2 {
    margin: 2px 0 0;
    color: var(--text-primary);
    font-size: 18px;
    font-weight: 600;
  }

  .close {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 5px;
    color: var(--icon-default);
  }

  .close:hover {
    background: var(--bg-hover);
  }

  .content {
    flex: 1;
    padding: 4px 24px 24px;
    overflow: auto;
  }
</style>
