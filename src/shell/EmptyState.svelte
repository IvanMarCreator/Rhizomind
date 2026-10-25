<!-- Пустое состояние области заметки: ни одна заметка не открыта. -->
<script lang="ts">
  import { t } from '../i18n/index.ts';
  import Icon from './Icon.svelte';
</script>

<div class="empty">
  <span class="mark"><Icon name="file-text" size={28} /></span>
  <p class="title">{t('shell.empty.title')}</p>
  <p class="hint">{t('shell.empty.hint')}</p>
</div>

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 100%;
    padding: 0 32px;
    text-align: center;
  }

  .mark {
    margin-bottom: 6px;
    color: var(--icon-muted);
  }

  .title {
    margin: 0;
    color: var(--text-secondary);
    font-size: 15px;
    font-weight: 600;
  }

  .hint {
    max-width: 36ch;
    margin: 0;
    color: var(--text-muted);
  }
</style>
