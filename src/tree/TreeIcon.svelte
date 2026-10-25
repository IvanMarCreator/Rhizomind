<!--
  Иконки дерева файлов — Lucide 1.61.0 (ISC, текст лицензии —
  src/shell/icons/Lucide-LICENSE.txt), контуры перенесены вручную, как в
  src/shell/Icon.svelte: шеврон папки, «Новая заметка», «Новая папка».
-->
<script lang="ts" module>
  export type TreeIconName = 'chevron-right' | 'square-pen' | 'folder-plus';
</script>

<script lang="ts">
  let { name, size = 14 }: { name: TreeIconName; size?: number } = $props();
</script>

<svg
  class="icon"
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
  {#if name === 'chevron-right'}
    <path d="m9 18 6-6-6-6" />
  {:else if name === 'square-pen'}
    <path d="M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
    <path
      d="M18.375 2.625a1 1 0 0 1 3 3l-9.013 9.014a2 2 0 0 1-.853.505l-2.873.84a.5.5 0 0 1-.62-.62l.84-2.873a2 2 0 0 1 .506-.852z"
    />
  {:else if name === 'folder-plus'}
    <path d="M12 10v6" />
    <path d="M9 13h6" />
    <path
      d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"
    />
  {/if}
</svg>

<style>
  .icon {
    display: block;
    flex: none;
  }
</style>
