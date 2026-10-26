<!-- Строка настройки (Э12): название, пояснение, метка выпуска, управление справа. -->
<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    title,
    hint,
    badge,
    disabled = false,
    id,
    children,
  }: {
    title: string;
    hint?: string | undefined;
    /** Метка отложенного параметра: «с 0.2», «в следующем выпуске». */
    badge?: string | undefined;
    disabled?: boolean;
    /** id названия: управление ссылается на него через aria-labelledby. */
    id?: string | undefined;
    children?: Snippet;
  } = $props();
</script>

<div class="row" class:disabled>
  <div class="text">
    <div class="title">
      <span {id}>{title}</span>
      {#if badge}<span class="badge">{badge}</span>{/if}
    </div>
    {#if hint}<p class="hint">{hint}</p>{/if}
  </div>
  {#if children}
    <div class="control">{@render children()}</div>
  {/if}
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 24px;
    padding: 14px 0;
    border-bottom: 1px solid var(--border-default);
  }

  .text {
    flex: 1;
    min-width: 0;
  }

  .title {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-primary);
    font-weight: 500;
  }

  .disabled .title,
  .disabled .hint {
    color: var(--text-faint);
  }

  .badge {
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--bg-active);
    color: var(--text-secondary);
    font-size: 11px;
    font-weight: 500;
  }

  .hint {
    margin: 3px 0 0;
    color: var(--text-muted);
    font-size: var(--font-size-caption);
    line-height: 17px;
  }

  .control {
    display: flex;
    flex: none;
    align-items: center;
    gap: 8px;
  }
</style>
