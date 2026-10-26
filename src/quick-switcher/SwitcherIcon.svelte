<!--
  Иконки быстрого перехода — Lucide 1.61.0 (ISC, текст лицензии —
  src/shell/icons/Lucide-LICENSE.txt), контуры перенесены вручную, как в
  src/shell/Icon.svelte. Отдельный файл, чтобы не пересекаться с задачами,
  которые одновременно добавляют иконки в оболочку.
-->
<script lang="ts">
  let { name, size = 16 }: { name: 'search' | 'plus' | 'corner-down-left'; size?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
>
  {#if name === 'search'}
    <path d="m21 21-4.34-4.34" />
    <circle cx="11" cy="11" r="8" />
  {:else if name === 'plus'}
    <path d="M5 12h14" />
    <path d="M12 5v14" />
  {:else}
    <path d="M20 4v7a4 4 0 0 1-4 4H4" />
    <path d="m9 10-5 5 5 5" />
  {/if}
</svg>

<style>
  svg {
    display: block;
    flex: none;
  }
</style>
