// Правая боковая панель (С2-3.1, решения 108 и 119, макет П-А): открыта ли,
// выбранный вид, ширина и порядок тегов. Само состояние живёт в модели
// раскладки окна (src/core/layout/layout.ts, поле `side_panel` в
// `.rhizomind/workspace.json`); модель панели меняет его через хозяина
// (оболочку окна) и решает, когда изменение пора записать в хранилище.
//
// Виды «Обратные ссылки» (0.2-2.2) и «Теги» (0.2-2.3) получают эту модель:
// число в заголовке вида ставит сам вид (`setCount`), порядок тегов —
// `tagOrder` / `setTagOrder`, открыть вид из редактора — `showView`/`reveal`.
import {
  clampRightPanelWidth,
  type RightPanel,
  type RightPanelView,
  type TagOrder,
} from '../core/layout/layout.ts';

export interface RightPanelHost {
  /** Состояние панели в раскладке окна. */
  readonly state: RightPanel;
  /** Новое состояние; `persist` — записать раскладку в хранилище. */
  update(next: RightPanel, persist: boolean): void;
}

/** Шаг ширины с клавиатуры, px. */
export const RESIZE_STEP = 10;

export class RightPanelModel {
  /** Число в заголовке вида; `null` — вид его ещё не дал (заголовок без числа). */
  counts = $state<Record<RightPanelView, number | null>>({ backlinks: null, tags: null });

  readonly #host: RightPanelHost;
  /** Перетаскивание границы: где начали и с какой ширины; `null` — не тащат. */
  #drag = $state.raw<{ x: number; width: number } | null>(null);

  constructor(host: RightPanelHost) {
    this.#host = host;
  }

  get open(): boolean {
    return this.#host.state.open;
  }

  get view(): RightPanelView {
    return this.#host.state.view;
  }

  get width(): number {
    return this.#host.state.width;
  }

  get tagOrder(): TagOrder {
    return this.#host.state.tagOrder;
  }

  /** Кнопка в строке над заметкой: открыть или закрыть панель. */
  toggle(): void {
    this.#set({ open: !this.open });
  }

  /**
   * Команда реестра «Обратные ссылки» / «Теги»: открыть панель на этом виде;
   * если панель уже открыта на нём — закрыть.
   */
  showView(view: RightPanelView): void {
    if (this.open && this.view === view) this.#set({ open: false });
    else this.#set({ open: true, view });
  }

  /** Открыть панель на виде, не закрывая её (переход из редактора, например по тегу). */
  reveal(view: RightPanelView): void {
    if (!this.open || this.view !== view) this.#set({ open: true, view });
  }

  /** Значок вида в заголовке панели. */
  selectView(view: RightPanelView): void {
    if (this.view !== view) this.#set({ view });
  }

  setTagOrder(order: TagOrder): void {
    if (this.tagOrder !== order) this.#set({ tagOrder: order });
  }

  setCount(view: RightPanelView, count: number | null): void {
    this.counts[view] = count;
  }

  /** Начало перетаскивания левой границы (`clientX` указателя). */
  beginResize(x: number): void {
    this.#drag = { x, width: this.width };
  }

  /** Граница влево — панель шире. Пока тащат, раскладка не записывается. */
  resizeTo(x: number): void {
    if (!this.#drag) return;
    const width = clampRightPanelWidth(this.#drag.width + this.#drag.x - x);
    if (width !== this.width) this.#host.update({ ...this.#host.state, width }, false);
  }

  /** Отпустили границу: ширина записывается, если изменилась. */
  endResize(): void {
    if (!this.#drag) return;
    const changed = this.#drag.width !== this.width;
    this.#drag = null;
    if (changed) this.#host.update({ ...this.#host.state }, true);
  }

  get resizing(): boolean {
    return this.#drag !== null;
  }

  /** Ширина с клавиатуры: `delta` > 0 — шире. */
  resizeBy(delta: number): void {
    const width = clampRightPanelWidth(this.width + delta);
    if (width !== this.width) this.#set({ width });
  }

  #set(patch: Partial<RightPanel>): void {
    this.#host.update({ ...this.#host.state, ...patch }, true);
  }
}
