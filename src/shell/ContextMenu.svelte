<!--
  Своё контекстное меню Rhizomind (решение 97; меню движка выключено): в
  дереве (С3.2) — пункты по месту щелчка, в тексте заметки и полях — правка
  текста (src/shell/textMenu.ts). Стрелки двигают фокус по активным пунктам,
  Enter выбирает, Esc и щелчок мимо закрывают; неактивные пункты серые.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import type { MenuItem } from './menu.ts';
  let {
    x,
    y,
    items,
    label,
    onClose,
  }: { x: number; y: number; items: MenuItem[]; label: string; onClose: () => void } = $props();

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
    // Фокус может вернуться туда, где щёлкнули (WebKitGTK ставит его после
    // события contextmenu): клавиши меню работают и тогда — Esc и Tab
    // закрывают, стрелки уводят фокус в меню, а не в дерево или поле.
    const onWindowKeydown = (event: KeyboardEvent) => {
      if (!menu || menu.contains(document.activeElement)) return;
      if (event.key === 'Escape' || event.key === 'Tab') {
        event.preventDefault();
        event.stopPropagation();
        onClose();
      } else if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
        event.preventDefault();
        event.stopPropagation();
        focusItem(event.key === 'ArrowUp' || event.key === 'End' ? -1 : 0);
      }
    };
    window.addEventListener('pointerdown', onPointerDown, true);
    window.addEventListener('keydown', onWindowKeydown, true);
    return () => {
      window.removeEventListener('pointerdown', onPointerDown, true);
      window.removeEventListener('keydown', onWindowKeydown, true);
    };
  });

  function buttons(): HTMLButtonElement[] {
    return menu ? [...menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not([aria-disabled="true"])')] : [];
  }

  function focusItem(index: number) {
    const all = buttons();
    if (all.length === 0) {
      // Все пункты неактивны: фокус на меню, чтобы работал Esc.
      menu?.focus();
      return;
    }
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
    if (item.disabled) return;
    onClose();
    item.run();
  }
</script>

<div
  bind:this={menu}
  class="menu"
  role="menu"
  tabindex="-1"
  aria-label={label}
  style:left={`${menuLeft}px`}
  style:top={`${menuTop}px`}
  {onkeydown}
>
  {#each items as item (item.id)}
    <button
      type="button"
      class="item"
      role="menuitem"
      tabindex="-1"
      aria-disabled={item.disabled ? 'true' : undefined}
      onclick={() => choose(item)}
    >
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
    /* Выше модальных окон (настройки — 30) и подсказок вики-ссылок (50). */
    z-index: 60;
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

  .item[aria-disabled='true'] {
    color: var(--text-faint);
    cursor: default;
  }

  .item[aria-disabled='true']:hover {
    background: none;
  }

  .label {
    flex: 1;
  }

  .shortcut {
    color: var(--text-faint);
    font-size: var(--font-size-caption);
  }
</style>
