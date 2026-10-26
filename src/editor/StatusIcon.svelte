<!--
  Иконки редактора — Lucide 1.61.0 (ISC, текст лицензии —
  src/shell/icons/Lucide-LICENSE.txt): `check`, `lock`, `circle-alert`.
  Контуры перенесены из пакета lucide-static вручную, без зависимости.
-->
<script lang="ts" module>
  export type StatusIconName = 'check' | 'lock' | 'circle-alert';
</script>

<script lang="ts">
  let { name, size = 14 }: { name: StatusIconName; size?: number } = $props();
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
  {#if name === 'check'}
    <path d="M20 6 9 17l-5-5" />
  {:else if name === 'lock'}
    <rect width="18" height="11" x="3" y="11" rx="2" ry="2" />
    <path d="M7 11V7a5 5 0 0 1 10 0v4" />
  {:else if name === 'circle-alert'}
    <circle cx="12" cy="12" r="10" />
    <line x1="12" x2="12" y1="8" y2="12" />
    <line x1="12" x2="12.01" y1="16" y2="16" />
  {/if}
</svg>

<style>
  svg {
    display: block;
    flex: none;
  }
</style>
