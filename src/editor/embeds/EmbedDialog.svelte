<script lang="ts">
  import { onMount } from 'svelte';
  import type { EmbedSource } from '../../api/index.ts';
  import { t } from '../../i18n/index.ts';
  import { label, previewInto } from './view.ts';
  import type { ShowEmbed } from './note.ts';
  let { source, onClose, onPreview }: { source: EmbedSource; onClose: () => void; onPreview: ShowEmbed } = $props();
  let content: HTMLDivElement, close: HTMLButtonElement;
  onMount(() => close.focus());
  $effect(() => previewInto(content, source, onPreview));
  function key(event: KeyboardEvent) {
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); onClose(); }
    if (event.key !== 'Tab') return;
    const items = [...(event.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>('button')];
    if ((!event.shiftKey && document.activeElement === items.at(-1)) || (event.shiftKey && document.activeElement === items[0])) {
      event.preventDefault(); (event.shiftKey ? items.at(-1) : items[0])?.focus();
    }
  }
</script>
<div class="embed-shade">
  <div class="embed-dialog" role="dialog" aria-modal="true" aria-label={label(source) || t('embed.preview')} tabindex="-1" onkeydown={key}>
    <header><strong>{label(source) || t('embed.preview')}</strong><button type="button" bind:this={close} onclick={onClose} aria-label={t('embed.close')}>×</button></header>
    <div class="embed-scroll" bind:this={content}></div>
  </div>
</div>
<style>
  .embed-shade { position: fixed; inset: 0; z-index: 50; display: grid; place-items: center; background: rgb(0 0 0 / 35%); }
  .embed-dialog { display: flex; flex-direction: column; max-width: min(80vw, 880px); min-width: min(320px, 90vw); max-height: 80vh; padding: 16px; border: 1px solid var(--border-strong); border-radius: 8px; background: var(--bg-popover); color: var(--text-primary); }
  header { display: flex; gap: 24px; align-items: center; justify-content: space-between; padding-bottom: 12px; overflow-wrap: anywhere; }
  button { cursor: pointer; font: inherit; color: inherit; background: transparent; border: none; font-size: 1.4em; }
  .embed-scroll { overflow: auto; }
</style>
