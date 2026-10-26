<!-- Клавиши сочетания «Ctrl + O» (Э12.5б). -->
<script lang="ts">
  import { chordParts } from './hotkeys.ts';

  let { chord, muted = false }: { chord: string; muted?: boolean } = $props();
</script>

<span class="keys" class:muted>
  {#each chordParts(chord) as part, i (i)}
    {#if i > 0}<span class="plus" aria-hidden="true">+</span>{/if}
    <kbd class="key">{part}</kbd>
  {/each}
</span>

<style>
  .keys {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }

  .key {
    min-width: 12px;
    padding: 1px 6px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    background: var(--bg-app);
    color: var(--text-primary);
    font-family: var(--font-ui);
    font-size: 11px;
    line-height: 17px;
    text-align: center;
  }

  .plus {
    color: var(--text-faint);
    font-size: 11px;
  }

  .muted .key {
    color: var(--text-faint);
    border-color: var(--border-default);
  }
</style>
