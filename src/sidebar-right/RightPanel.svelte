<!--
  Правая боковая панель (С2-3.1, решения 108 и 119, макет П-А — кадры 66-2 и
  65-2): вверху значки видов «Обратные ссылки» (link) и «Теги» (hash), под
  ними название вида заглавными и число — как «ФАЙЛЫ» слева; ширина — левой
  границей (мышью и стрелками). Содержимое видов приходит снаружи
  (`backlinks`, `tags`): виды — задачи 0.2-2.2 и 0.2-2.3; число в заголовке
  ставит вид через `panel.setCount`. В 0.4 значки станут вкладками панели.
-->
<script lang="ts">
  import { onDestroy, type Snippet } from 'svelte';
  import { RIGHT_PANEL_WIDTH, type RightPanelView } from '../core/layout/layout.ts';
  import { t, type MessageKey } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import { RESIZE_STEP, type RightPanelModel } from './panel.svelte.ts';

  let {
    panel,
    shortcuts = { backlinks: null, tags: null },
    backlinks,
    tags,
  }: {
    panel: RightPanelModel;
    /** Сочетания команд видов для подсказок (`Ctrl+Shift+B`); `null` — не назначено. */
    shortcuts?: Record<RightPanelView, string | null>;
    /** Вид «Обратные ссылки» (0.2-2.2). */
    backlinks?: Snippet;
    /** Вид «Теги» (0.2-2.3). */
    tags?: Snippet;
  } = $props();

  const VIEWS: { view: RightPanelView; label: MessageKey; icon: 'link' | 'hash' }[] = [
    { view: 'backlinks', label: 'rightPanel.view.backlinks', icon: 'link' },
    { view: 'tags', label: 'rightPanel.view.tags', icon: 'hash' },
  ];

  const uid = $props.id();
  let buttons: HTMLButtonElement[] = $state([]);

  const LABELS: Record<RightPanelView, MessageKey> = {
    backlinks: 'rightPanel.view.backlinks',
    tags: 'rightPanel.view.tags',
  };
  const count = $derived(panel.counts[panel.view]);
  const widthCss = $derived(`${panel.width}px`);

  function tooltip(view: RightPanelView, label: MessageKey): string {
    const shortcut = shortcuts[view];
    return shortcut ? t('rightPanel.viewWith', { view: t(label), shortcut }) : t(label);
  }

  /** Значки видов — одна остановка Tab; ← и → выбирают вид (как переключатель режима). */
  function onTabKeydown(event: KeyboardEvent): void {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    event.preventDefault();
    const index = VIEWS.findIndex((item) => item.view === panel.view);
    const next = (index + (event.key === 'ArrowLeft' ? -1 : 1) + VIEWS.length) % VIEWS.length;
    const item = VIEWS[next];
    if (!item) return;
    panel.selectView(item.view);
    buttons[next]?.focus();
  }

  // ---- Ширина ---------------------------------------------------------------

  function onPointerDown(event: PointerEvent): void {
    if (event.button !== 0) return;
    event.preventDefault();
    panel.beginResize(event.clientX);
    // Не <svelte:window>: проверка сети (tools/check-no-network) не может
    // доказать безопасность объекта window, переданного значением.
    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', onPointerUp);
    window.addEventListener('pointercancel', onPointerUp);
  }

  function onPointerMove(event: PointerEvent): void {
    panel.resizeTo(event.clientX);
  }

  function onPointerUp(): void {
    window.removeEventListener('pointermove', onPointerMove);
    window.removeEventListener('pointerup', onPointerUp);
    window.removeEventListener('pointercancel', onPointerUp);
    panel.endResize();
  }

  // Панель закрыли посреди перетаскивания (командой): слушатели снимаются, ширина записывается.
  onDestroy(() => {
    if (panel.resizing) onPointerUp();
  });

  function onResizeKeydown(event: KeyboardEvent): void {
    if (event.key === 'ArrowLeft') panel.resizeBy(RESIZE_STEP);
    else if (event.key === 'ArrowRight') panel.resizeBy(-RESIZE_STEP);
    else return;
    event.preventDefault();
  }
</script>

<aside
  class="right-panel"
  class:resizing={panel.resizing}
  aria-label={t('rightPanel.label')}
  style:width={widthCss}
>
  <!-- Разделитель с фокусом и значением — элемент управления (WAI-ARIA, window splitter). -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="resize"
    role="separator"
    tabindex="0"
    aria-label={t('rightPanel.resize')}
    aria-orientation="vertical"
    aria-valuenow={panel.width}
    aria-valuemin={RIGHT_PANEL_WIDTH.min}
    aria-valuemax={RIGHT_PANEL_WIDTH.max}
    onpointerdown={onPointerDown}
    onkeydown={onResizeKeydown}
  ></div>

  <div class="views" role="tablist" aria-label={t('rightPanel.views')}>
    {#each VIEWS as item, i (item.view)}
      <button
        bind:this={buttons[i]}
        id="{uid}-tab-{item.view}"
        type="button"
        role="tab"
        class="view-button"
        class:active={panel.view === item.view}
        aria-selected={panel.view === item.view}
        aria-controls="{uid}-view"
        aria-label={t(item.label)}
        title={tooltip(item.view, item.label)}
        tabindex={panel.view === item.view ? 0 : -1}
        onclick={() => panel.selectView(item.view)}
        onkeydown={onTabKeydown}
      >
        <Icon name={item.icon} size={15} />
      </button>
    {/each}
  </div>

  <h2 class="heading">
    <span class="name">{t(LABELS[panel.view])}</span>
    {#if count !== null}
      <span class="count">{count}</span>
    {/if}
  </h2>

  <div
    class="body"
    id="{uid}-view"
    role="tabpanel"
    aria-labelledby="{uid}-tab-{panel.view}"
    data-view={panel.view}
  >
    {#if panel.view === 'backlinks'}
      {@render backlinks?.()}
    {:else}
      {@render tags?.()}
    {/if}
  </div>
</aside>

<style>
  /* tokens.md, «0.2»: ширина 300 по умолчанию, фон `bg/sidebar`, граница слева 1 px. */
  .right-panel {
    position: relative;
    display: flex;
    flex: none;
    flex-direction: column;
    min-width: 0;
    background: var(--bg-sidebar);
    border-left: 1px solid var(--border-default);
    box-sizing: border-box;
  }

  .right-panel.resizing {
    cursor: col-resize;
    user-select: none;
  }

  /* Граница для перетаскивания: полоса 6 px поверх левой линии панели. */
  .resize {
    position: absolute;
    top: 0;
    bottom: 0;
    left: -4px;
    z-index: 5;
    width: 6px;
    cursor: col-resize;
  }

  .resize::after {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 2px;
    width: 2px;
    background: transparent;
    content: '';
    transition: background 0.1s ease;
  }

  .resize:hover::after,
  .resizing .resize::after,
  .resize:focus-visible::after {
    background: var(--accent);
  }

  .resize:focus-visible {
    outline: none;
  }

  /* Заголовок П-А: высота 38, отступы 8, кнопки 26 × 26, иконка 15, скругление 5. */
  .views {
    display: flex;
    flex: none;
    align-items: center;
    gap: 2px;
    height: 38px;
    padding: 0 8px;
    box-sizing: border-box;
  }

  .view-button {
    display: grid;
    place-items: center;
    width: var(--note-header-button);
    height: var(--note-header-button);
    border-radius: 5px;
    color: var(--icon-default);
  }

  .view-button:hover:not(.active) {
    background: var(--bg-hover);
    cursor: pointer;
  }

  .view-button.active {
    background: var(--bg-active);
    color: var(--text-primary);
  }

  .view-button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  /* Название вида: 11 Semi Bold заглавными, разрядка 6 %, `text/muted`; число `text/faint`; высота 30, слева 14. */
  .heading {
    display: flex;
    flex: none;
    align-items: center;
    gap: 6px;
    height: 30px;
    margin: 0;
    padding: 0 10px 0 14px;
    overflow: hidden;
    font-size: var(--font-size-heading);
    line-height: 14px;
    white-space: nowrap;
    box-sizing: border-box;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text-muted);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .count {
    color: var(--text-faint);
    font-weight: 500;
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
</style>
