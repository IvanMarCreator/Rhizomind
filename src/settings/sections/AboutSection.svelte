<!-- Раздел «О программе» (Э12.6): версия, редакция, лицензия, коротко о безопасности. -->
<script lang="ts">
  import { t } from '../../i18n/index.ts';
  import Icon from '../../shell/Icon.svelte';
  import { APP_VERSION } from '../../vaults/version.ts';
  import Row from '../Row.svelte';
</script>

<div class="product">
  <span class="logo" aria-hidden="true"><Icon name="waypoints" size={20} /></span>
  <div>
    <div class="name">{t('settings.about.name')}</div>
    <div class="tagline">{t('settings.about.tagline')}</div>
  </div>
</div>
<Row title={t('settings.about.version')}><span class="value">{APP_VERSION}</span></Row>
<Row title={t('settings.about.edition')} hint={t('settings.about.editionHint')}>
  <span class="value">{t('settings.about.editionValue')}</span>
</Row>
<Row title={t('settings.about.license')}><span class="value">{t('settings.about.licenseValue')}</span></Row>
<section class="security" aria-labelledby="settings-security">
  <h3 id="settings-security"><Icon name="lock" size={14} />{t('settings.about.security')}</h3>
  <ul>
    <li>{t('settings.about.security.network')}</li>
    <li>{t('settings.about.security.storage')}</li>
    <li>{t('settings.about.security.links')}</li>
  </ul>
</section>

<style>
  .product {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 0 12px;
  }

  .logo {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 8px;
    background: var(--accent-subtle);
    color: var(--accent-text);
  }

  .name {
    color: var(--text-primary);
    font-size: 16px;
    font-weight: 600;
  }

  .tagline {
    color: var(--text-muted);
    font-size: var(--font-size-caption);
  }

  .value {
    color: var(--text-primary);
    font-weight: 500;
  }

  .security {
    margin-top: 14px;
    padding: 12px 14px;
    border: 1px solid var(--border-default);
    border-radius: 8px;
    background: var(--bg-sidebar);
  }

  h3 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 0 6px;
    color: var(--text-primary);
    font-size: var(--font-size-ui);
    font-weight: 600;
  }

  h3 :global(.icon) {
    color: var(--accent-text);
  }

  ul {
    margin: 0;
    padding-left: 18px;
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
    line-height: 20px;
  }
</style>
