<!-- Окно не может работать (другая версия API, отказ моста): понятное сообщение вместо пустого окна. -->
<script lang="ts">
  import { t, type MessageKey } from '../i18n/index.ts';
  import Icon from './Icon.svelte';

  let { title, message }: { title: MessageKey; message: string } = $props();
</script>

<div class="screen">
  <div class="card" role="alert">
    <h1 class="title">
      <span class="badge"><Icon name="triangle-alert" size={16} /></span>
      {t(title)}
    </h1>
    <p class="message">{message}</p>
  </div>
</div>

<style>
  .screen {
    display: grid;
    place-items: center;
    height: 100%;
    padding: 20px;
    box-sizing: border-box;
    background: var(--bg-sidebar);
  }

  .card {
    width: min(460px, 100%);
    padding: 20px;
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    background: var(--bg-popover);
    box-sizing: border-box;
  }

  .title {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0;
    color: var(--text-primary);
    font-size: 15px;
    font-weight: 600;
  }

  .badge {
    display: grid;
    place-items: center;
    flex: none;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: var(--status-warning-subtle);
    color: var(--status-warning);
  }

  .message {
    margin: 12px 0 0;
    color: var(--text-secondary);
    line-height: 20px;
  }
</style>
