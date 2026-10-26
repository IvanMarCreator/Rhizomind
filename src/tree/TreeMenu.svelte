<!--
  Контекстное меню дерева (С3.2): пункты по месту щелчка — на папке, на файле
  или на пустом месте. Стрелки двигают фокус, Esc и щелчок мимо закрывают.
-->
<script lang="ts" module>
  export interface MenuItem {
    id: string;
    label: string;
    /** Клавиша-подсказка справа (F2, Delete). */
    shortcut?: string | undefined;
    run: () => void;
  }
</script>

<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../i18n/index.ts';

  let { x, y, items, onClose }: { x: number; y: number; items: MenuItem[]; onClose: () => void } = $props();

  let menu: HTMLDivElement | undefined = $state();
  let menuLeft = $state(0);
  let menuTop = $state(0);

  onMount(() => {
    menuLeft = x;
    menuTop = y;
    if (menu) {
      // Меню не выходит за край окна.
      const { width, height } = menu.getBoundingClientRect();
      menuLeft = Math.max(0, Math.min(x, document.documentElement.clientWidth - width - 4));
      menuTop = Math.max(0, Math.min(y, document.documentElement.clientHeight - height - 4));
    }
    focusItem(0);
    const onPointerDown = (event: PointerEvent) => {
      if (menu && !menu.contains(event.target as Node)) onClose();
    };
    window.addEventListener('pointerdown', onPointerDown, true);
    return () => window.removeEventListener('pointerdown', onPointerDown, true);
  });

  function buttons(): HTMLButtonElement[] {
    return menu ? [...menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')] : [];
  }

  function focusItem(index: number) {
    const all = buttons();
    if (all.length === 0) return;
    all[(index + all.length) % all.length]?.focus();
  }

  function onkeydown(event: KeyboardEvent) {
    const current = buttons().indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === 'Escape' || event.key === 'Tab') {
      event.preventDefault();
      onClose();
    } else if (event.key === 'ArrowDown') {
      event.preventDefault();
      focusItem(current + 1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      focusItem(current - 1);
    } else if (event.key === 'Home') {
      event.preventDefault();
      focusItem(0);
    } else if (event.key === 'End') {
      event.preventDefault();
      focusItem(-1);
    }
  }

  function choose(item: MenuItem) {
    onClose();
    item.run();
  }
</script>

<div
  bind:this={menu}
  class="menu"
  role="menu"
  tabindex="-1"
  aria-label={t('tree.menu.label')}
  style:left={`${menuLeft}px`}
  style:top={`${menuTop}px`}
  {onkeydown}
>
  {#each items as item (item.id)}
    <button type="button" class="item" role="menuitem" tabindex="-1" onclick={() => choose(item)}>
      <span class="label">{item.label}</span>
      {#if item.shortcut}
        <span class="shortcut">{item.shortcut}</span>
      {/if}
    </button>
  {/each}
</div>

<style>
  .menu {
    position: fixed;
    z-index: 20;
    min-width: 200px;
    padding: 4px;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    background: var(--bg-popover);
    box-shadow: 0 8px 24px rgb(0 0 0 / 14%);
    box-sizing: border-box;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 16px;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border-radius: 5px;
    color: var(--text-primary);
    text-align: left;
  }

  .item:hover,
  .item:focus-visible {
    outline: none;
    background: var(--bg-hover);
  }

  .label {
    flex: 1;
  }

  .shortcut {
    color: var(--text-faint);
    font-size: var(--font-size-caption);
  }
</style>
