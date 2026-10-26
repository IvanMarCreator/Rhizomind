<!--
  Окно «Управление хранилищами» (С3.1, решение владельца 58; макеты
  Э11а–Э11в): слева — известные хранилища, справа — «Создать хранилище» и
  «Открыть папку как хранилище». При первом запуске список пуст (Э11б) — это
  же окно служит окном приветствия. Отдельное окно приложения без хранилища:
  его создаёт ядро (src-tauri/src/windows/mod.rs, open_manager).
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import CreateVault from './CreateVault.svelte';
  import { VaultManagerModel } from './manager.svelte.ts';
  import VaultRow from './VaultRow.svelte';
  import { APP_VERSION } from './version.ts';

  let { onNotice }: { onNotice: (text: string) => void } = $props();

  const model = createModel();

  function createModel(): VaultManagerModel {
    return new VaultManagerModel((text) => onNotice(text));
  }

  onMount(() => {
    void model.load();
  });
</script>

<div class="manager" data-testid="vault-manager">
  <section class="vaults" aria-labelledby="vault-manager-list">
    <h2 class="heading" id="vault-manager-list">{t('vaults.manager.list')}</h2>
    {#if model.vaults.length === 0}
      <div class="empty">
        <span class="empty-icon"><Icon name="library" size={32} /></span>
        <p class="empty-title">{t('vaults.manager.empty.title')}</p>
        <p class="empty-hint">{t('vaults.manager.empty.hint')}</p>
      </div>
    {:else}
      <ul class="list">
        {#each model.vaults as vault (vault.id)}
          <VaultRow
            {vault}
            disabled={model.busy}
            onOpen={() => void model.open(vault.id)}
            onRemove={() => void model.remove(vault)}
          />
        {/each}
      </ul>
    {/if}
  </section>

  <section class="side">
    <div class="sign">
      <span class="mark"><Icon name="waypoints" size={26} /></span>
      <div>
        <h1 class="app">{t('app.name')}</h1>
        <p class="tagline">{t('vaults.manager.tagline')}</p>
      </div>
    </div>

    {#if model.view === 'create'}
      <CreateVault {model} />
    {:else}
      <div class="action">
        <div class="action-text">
          <p class="action-title">{t('vaults.manager.create.title')}</p>
          <p class="action-hint">{t('vaults.manager.create.hint')}</p>
        </div>
        <button type="button" class="primary" disabled={model.busy} onclick={() => model.showCreate()}>
          {t('vaults.manager.create.button')}
        </button>
      </div>
      <div class="action">
        <div class="action-text">
          <p class="action-title">{t('vaults.manager.openFolder.title')}</p>
          <p class="action-hint">{t('vaults.manager.openFolder.hint')}</p>
        </div>
        <button
          type="button"
          class="secondary"
          disabled={model.busy}
          aria-label={t('vaults.manager.openFolder.title')}
          onclick={() => void model.openFolder()}
        >
          {t('vaults.manager.openFolder.button')}
        </button>
      </div>
    {/if}

    <div class="spacer"></div>
    <footer class="footer">
      <p class="offline">
        <span class="lock"><Icon name="lock" size={14} /></span>
        {t('vaults.manager.offline')}
      </p>
      <p class="version">{t('vaults.manager.version', { version: APP_VERSION })}</p>
    </footer>
  </section>
</div>

<style>
  .manager {
    display: flex;
    height: 100%;
    background: var(--bg-app);
  }

  .vaults {
    display: flex;
    flex-direction: column;
    flex: 0 1 400px;
    min-width: 280px;
    padding: 24px 16px 16px;
    border-right: 1px solid var(--border-default);
    background: var(--bg-sidebar);
    box-sizing: border-box;
    overflow: auto;
  }

  .heading {
    margin: 0;
    padding: 0 10px 10px;
    color: var(--text-muted);
    font-size: var(--font-size-heading);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    flex: 1;
    padding: 0 24px 40px;
    text-align: center;
  }

  .empty-icon {
    color: var(--icon-default);
  }

  .empty-title {
    margin: 0;
    color: var(--text-secondary);
    font-size: 14px;
    font-weight: 500;
  }

  .empty-hint {
    margin: 0;
    color: var(--text-muted);
    font-size: 12px;
    line-height: 17px;
  }

  .side {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    padding: 40px 40px 24px;
    overflow: auto;
  }

  .sign {
    display: flex;
    align-items: center;
    gap: 14px;
    padding-bottom: 28px;
  }

  .mark {
    display: grid;
    place-items: center;
    flex: none;
    width: 44px;
    height: 44px;
    border-radius: 11px;
    background: var(--accent-subtle);
    color: var(--accent);
  }

  .app {
    margin: 0;
    color: var(--text-primary);
    font-size: 22px;
    font-weight: 700;
    line-height: normal;
  }

  .tagline {
    margin: 3px 0 0;
    color: var(--text-muted);
    font-size: 13px;
  }

  .action {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 16px 0;
    border-bottom: 1px solid var(--border-default);
  }

  .action-text {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
    min-width: 0;
  }

  .action-title {
    margin: 0;
    color: var(--text-primary);
    font-size: 14px;
    font-weight: 600;
  }

  .action-hint {
    margin: 0;
    color: var(--text-muted);
    font-size: 12px;
    line-height: 17px;
  }

  .primary,
  .secondary {
    flex: none;
    height: 32px;
    padding: 0 14px;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 500;
  }

  .primary {
    background: var(--accent);
    color: var(--text-on-accent);
  }

  .primary:hover:not(:disabled) {
    filter: brightness(1.08);
  }

  .secondary {
    border: 1px solid var(--border-strong);
    background: var(--bg-app);
    color: var(--text-primary);
  }

  .secondary:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .primary:disabled,
  .secondary:disabled {
    opacity: 0.45;
  }

  .spacer {
    flex: 1;
    min-height: 24px;
  }

  .footer {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .offline {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 0;
    color: var(--text-muted);
    font-size: 12px;
    line-height: 17px;
  }

  .lock {
    padding-top: 1px;
    color: var(--icon-muted);
  }

  .version {
    margin: 0;
    color: var(--text-faint);
    font-size: 12px;
  }
</style>
