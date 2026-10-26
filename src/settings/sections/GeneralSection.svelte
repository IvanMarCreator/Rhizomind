<!-- Раздел «Общие» (Э12.1): хранилище (только чтение), язык интерфейса — в 0.5. -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { core, type CurrentVault } from '../../api/index.ts';
  import { t } from '../../i18n/index.ts';
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
  <span class="fixed" aria-disabled="true">{t('settings.language.russian')}</span>
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
    min-width: 120px;
    padding: 5px 10px;
    border: 1px solid var(--border-default);
    border-radius: 5px;
    color: var(--text-secondary);
  }
</style>
