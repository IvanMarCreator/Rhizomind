<!-- Раздел «Общие» (Э12.1): хранилище (только чтение), язык интерфейса — в 0.5. -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { core, type CurrentVault } from '../../api/index.ts';
  import { t } from '../../i18n/index.ts';
  import Icon from '../../shell/Icon.svelte';
  import Row from '../Row.svelte';

  let { vault }: { vault: CurrentVault } = $props();

  /** Полный путь к папке — из списка хранилищ; не прочитался — только имя. */
  let path = $state<string | null>(null);

  onMount(() => {
    core
      .vaultList()
      .then(({ vaults }) => {
        path = vaults.find((item) => item.id === vault.id)?.path ?? null;
      })
      .catch(() => undefined);
  });
</script>

<Row
  title={t('settings.vault.title')}
  hint={path ? t('settings.vault.value', { name: vault.name, path }) : vault.name}
/>
<Row title={t('settings.language.title')} hint={t('settings.language.hint')}>
  <span class="badge">{t('settings.language.badge')}</span>
  <span class="fixed" aria-disabled="true">
    <span>{t('settings.language.russian')}</span>
    <Icon name="chevron-down" size={14} />
  </span>
</Row>

<style>
  .badge {
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--bg-active);
    color: var(--text-secondary);
    font-size: 11px;
  }

  .fixed {
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-width: 140px;
    height: 28px;
    padding: 0 8px;
    box-sizing: border-box;
    border: 1px solid var(--border-default);
    border-radius: 5px;
    color: var(--text-secondary);
    cursor: default;
  }
</style>
