<script lang="ts">
  import { t } from './i18n/index.ts';
</script>

<main>
  <h1>{t('app.name')}</h1>
</main>

<style>
  main {
    display: grid;
    place-items: center;
    height: 100vh;
  }

  h1 {
    margin: 0;
    font-size: 28px;
    font-weight: 600;
  }
</style>
