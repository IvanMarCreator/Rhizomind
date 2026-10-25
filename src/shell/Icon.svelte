<!--
  Иконки Lucide 1.61.0 (ISC, текст лицензии — icons/Lucide-LICENSE.txt):
  контуры перенесены из пакета lucide-static вручную, без загрузки из сети и
  без зависимости. Толщина линии 2 при 24 px, масштабируется с размером.
-->
<script lang="ts" module>
  export type IconName =
    | 'files'
    | 'square-arrow-right'
    | 'settings'
    | 'arrow-left'
    | 'arrow-right'
    | 'book-open'
    | 'pencil'
    | 'chevrons-up-down'
    | 'x'
    | 'file-text'
    | 'triangle-alert';
</script>

<script lang="ts">
  let { name, size = 18 }: { name: IconName; size?: number } = $props();
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
  {#if name === 'files'}
    <path d="M15 2h-4a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V8" />
    <path d="M16.706 2.706A2.4 2.4 0 0 0 15 2v5a1 1 0 0 0 1 1h5a2.4 2.4 0 0 0-.706-1.706z" />
    <path d="M5 7a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h8a2 2 0 0 0 1.732-1" />
  {:else if name === 'square-arrow-right'}
    <rect width="18" height="18" x="3" y="3" rx="2" />
    <path d="M8 12h8" />
    <path d="m12 16 4-4-4-4" />
  {:else if name === 'settings'}
    <path
      d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915"
    />
    <circle cx="12" cy="12" r="3" />
  {:else if name === 'arrow-left'}
    <path d="m12 19-7-7 7-7" />
    <path d="M19 12H5" />
  {:else if name === 'arrow-right'}
    <path d="M5 12h14" />
    <path d="m12 5 7 7-7 7" />
  {:else if name === 'book-open'}
    <path d="M12 5v16" />
    <path
      d="M20.001 19A2 2 0 0022 17V5a2 2 0 00-1.999-2L16 3.002A5 5 0 0012 5a5 5 0 00-4-2H4a2 2 0 00-2 2v12a2 2 0 001.999 2H8a5 5 0 014 2 5 5 0 014-2z"
    />
  {:else if name === 'pencil'}
    <path
      d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"
    />
    <path d="m15 5 4 4" />
  {:else if name === 'chevrons-up-down'}
    <path d="m7 15 5 5 5-5" />
    <path d="m7 9 5-5 5 5" />
  {:else if name === 'x'}
    <path d="M18 6 6 18" />
    <path d="m6 6 12 12" />
  {:else if name === 'file-text'}
    <path
      d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"
    />
    <path d="M14 2v5a1 1 0 0 0 1 1h5" />
    <path d="M10 9H8" />
    <path d="M16 13H8" />
    <path d="M16 17H8" />
  {:else if name === 'triangle-alert'}
    <path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3" />
    <path d="M12 9v4" />
    <path d="M12 17h.01" />
  {/if}
</svg>

<style>
  .icon {
    display: block;
    flex: none;
  }
</style>
