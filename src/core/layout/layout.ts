// Модель раскладки v1 (ADR 0003, С5.3): дерево разбиений и панелей с
// вкладками. Формат JSON свой, с номером версии; ядро хранит объект как есть
// (`workspace_read` / `workspace_write`, `.rhizomind/`). В 0.1 окно рисует
// одну панель — активную; разбиения читаются, но не создаются (с 0.4).
import { LIMITS } from '../../api/limits.ts';
import type { VaultPath, Workspace } from '../../api/index.ts';

export const LAYOUT_FORMAT = 'rhizomind-layout';
export const LAYOUT_VERSION = 1;
/** Предел вложенности разбиений при чтении: защита от испорченного файла. */
const MAX_DEPTH = 16;

/** Вкладка с заметкой. */
export interface NoteTab {
  type: 'note';
  path: VaultPath;
}

export type Tab = NoteTab;

/** Панель: вкладки и индекс активной (-1 — вкладок нет). */
export interface Pane {
  type: 'pane';
  id: string;
  tabs: Tab[];
  active: number;
}

/** Разбиение: части рядом (`row`) или друг под другом (`column`), доли размеров. */
export interface Split {
  type: 'split';
  direction: 'row' | 'column';
  sizes: number[];
  children: LayoutNode[];
}

export type LayoutNode = Pane | Split;

/** Вид правой боковой панели (С2-3.1, решение 108); в 0.4 виды станут обычными панелями. */
export type RightPanelView = 'backlinks' | 'tags';
/** Порядок списка тегов: по имени или по числу заметок (С2-3.3). */
export type TagOrder = 'name' | 'count';

/** Правая боковая панель: открыта ли, какой вид выбран, ширина в px, порядок тегов. */
export interface RightPanel {
  open: boolean;
  view: RightPanelView;
  width: number;
  tagOrder: TagOrder;
}

/** Пределы ширины правой панели (tokens.md, «0.2»: 300 по умолчанию). */
export const RIGHT_PANEL_WIDTH = { min: 220, max: 560, initial: 300 } as const;

export interface Layout {
  version: typeof LAYOUT_VERSION;
  root: LayoutNode;
  activePane: string;
  /** Поле 0.2; в файле — `side_panel`. В раскладке 0.1 его нет — тогда по умолчанию. */
  rightPanel: RightPanel;
}

/** Результат чтения: `recovered` — сохранённая раскладка испорчена и заменена. */
export interface ParsedLayout {
  layout: Layout;
  recovered: boolean;
}

/** Раскладка в JSON больше предела ядра (`LIMITS.max_workspace_bytes`). */
export class LayoutTooLarge extends Error {
  readonly bytes: number;

  constructor(bytes: number) {
    super(`Раскладка ${bytes} байт больше предела ${LIMITS.max_workspace_bytes}`);
    this.name = 'LayoutTooLarge';
    this.bytes = bytes;
  }
}

export function defaultRightPanel(): RightPanel {
  return { open: false, view: 'backlinks', width: RIGHT_PANEL_WIDTH.initial, tagOrder: 'name' };
}

export function defaultLayout(): Layout {
  return {
    version: LAYOUT_VERSION,
    root: { type: 'pane', id: 'pane-1', tabs: [], active: -1 },
    activePane: 'pane-1',
    rightPanel: defaultRightPanel(),
  };
}

/** Ширина правой панели в пределах `RIGHT_PANEL_WIDTH`, целым числом px. */
export function clampRightPanelWidth(width: number): number {
  if (!Number.isFinite(width)) return RIGHT_PANEL_WIDTH.initial;
  return Math.round(Math.min(RIGHT_PANEL_WIDTH.max, Math.max(RIGHT_PANEL_WIDTH.min, width)));
}

/** Раскладка с новым состоянием правой панели (ширина приводится к пределам). */
export function withRightPanel(layout: Layout, panel: RightPanel): Layout {
  return { ...layout, rightPanel: { ...panel, width: clampRightPanelWidth(panel.width) } };
}

function findPane(node: LayoutNode, id: string): Pane | null {
  if (node.type === 'pane') return node.id === id ? node : null;
  for (const child of node.children) {
    const found = findPane(child, id);
    if (found) return found;
  }
  return null;
}

/** Активная панель (по построению она всегда есть). */
export function activePane(layout: Layout): Pane {
  const pane = findPane(layout.root, layout.activePane);
  if (!pane) throw new Error(`Нет панели ${layout.activePane}`);
  return pane;
}

function replacePane(node: LayoutNode, pane: Pane): LayoutNode {
  if (node.type === 'pane') return node.id === pane.id ? pane : node;
  return { ...node, children: node.children.map((child) => replacePane(child, pane)) };
}

/** Открывает заметку в активной панели. В 0.1 у панели одна вкладка: она заменяется. */
export function openNote(layout: Layout, path: VaultPath): Layout {
  const pane: Pane = { ...activePane(layout), tabs: [{ type: 'note', path: [...path] }], active: 0 };
  return { ...layout, root: replacePane(layout.root, pane) };
}

/** Закрывает заметку активной панели (заметка удалена). В 0.1 у панели одна вкладка. */
export function closeNote(layout: Layout): Layout {
  const pane: Pane = { ...activePane(layout), tabs: [], active: -1 };
  return { ...layout, root: replacePane(layout.root, pane) };
}

/** Объект для `workspace_write`; больше 1 МБ в JSON — `LayoutTooLarge`. */
export function serializeLayout(layout: Layout): Workspace {
  const workspace: Workspace = {
    format: LAYOUT_FORMAT,
    version: layout.version,
    root: layout.root,
    active_pane: layout.activePane,
    side_panel: {
      open: layout.rightPanel.open,
      view: layout.rightPanel.view,
      width: layout.rightPanel.width,
      tag_order: layout.rightPanel.tagOrder,
    },
  };
  const bytes = new TextEncoder().encode(JSON.stringify(workspace)).length;
  if (bytes > LIMITS.max_workspace_bytes) throw new LayoutTooLarge(bytes);
  return workspace;
}

// ---- Чтение ----------------------------------------------------------------

class Invalid extends Error {}

function fail(): never {
  throw new Invalid();
}

function record(value: unknown): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) fail();
  return value as Record<string, unknown>;
}

function text(value: unknown): string {
  if (typeof value !== 'string' || value.length === 0 || value.length > LIMITS.max_component_utf16) fail();
  return value;
}

function vaultPath(value: unknown): VaultPath {
  if (!Array.isArray(value) || value.length === 0 || value.length > LIMITS.max_path_components) fail();
  return value.map((part) => {
    const name = text(part);
    if (name === '.' || name === '..' || /[/\\]/.test(name)) fail();
    return name;
  });
}

function tab(value: unknown): Tab {
  const raw = record(value);
  if (raw.type !== 'note') fail();
  return { type: 'note', path: vaultPath(raw.path) };
}

function node(value: unknown, depth: number, ids: Set<string>): LayoutNode {
  if (depth > MAX_DEPTH) fail();
  const raw = record(value);
  if (raw.type === 'pane') {
    const id = text(raw.id);
    if (ids.has(id) || !Array.isArray(raw.tabs)) fail();
    ids.add(id);
    const tabs = raw.tabs.map(tab);
    const active = raw.active;
    if (!Number.isInteger(active) || (active as number) < -1 || (active as number) >= tabs.length) fail();
    if (tabs.length > 0 && active === -1) fail();
    return { type: 'pane', id, tabs, active: active as number };
  }
  if (raw.type === 'split') {
    if (raw.direction !== 'row' && raw.direction !== 'column') fail();
    if (!Array.isArray(raw.children) || !Array.isArray(raw.sizes)) fail();
    if (raw.children.length === 0 || raw.children.length !== raw.sizes.length) fail();
    const sizes = raw.sizes.map((size) => (typeof size === 'number' && size > 0 && Number.isFinite(size) ? size : fail()));
    const children = raw.children.map((child) => node(child, depth + 1, ids));
    return { type: 'split', direction: raw.direction, sizes, children };
  }
  fail();
}

/**
 * Поле правой панели. Нет поля (раскладка 0.1) или оно испорчено — панель по
 * умолчанию, без пометки о сбое: панель — удобство, заметки важнее.
 */
function rightPanel(value: unknown): RightPanel {
  if (value === undefined) return defaultRightPanel();
  try {
    const raw = record(value);
    if (typeof raw.open !== 'boolean') fail();
    if (raw.view !== 'backlinks' && raw.view !== 'tags') fail();
    if (typeof raw.width !== 'number' || !Number.isFinite(raw.width)) fail();
    if (raw.tag_order !== 'name' && raw.tag_order !== 'count') fail();
    return { open: raw.open, view: raw.view, width: clampRightPanelWidth(raw.width), tagOrder: raw.tag_order };
  } catch (error) {
    if (error instanceof Invalid) return defaultRightPanel();
    throw error;
  }
}

/**
 * Раскладка из `workspace_read`. `null` (раскладки ещё нет) — по умолчанию.
 * Испорченная, чужого формата или другой версии — тоже по умолчанию, с
 * пометкой `recovered`: окно должно открыться в любом случае.
 */
export function parseLayout(saved: unknown): ParsedLayout {
  if (saved === null || saved === undefined) return { layout: defaultLayout(), recovered: false };
  try {
    const raw = record(saved);
    if (raw.format !== LAYOUT_FORMAT || raw.version !== LAYOUT_VERSION) fail();
    const root = node(raw.root, 0, new Set());
    const active = text(raw.active_pane);
    if (!findPane(root, active)) fail();
    const layout: Layout = { version: LAYOUT_VERSION, root, activePane: active, rightPanel: rightPanel(raw.side_panel) };
    return { layout, recovered: false };
  } catch (error) {
    if (error instanceof Invalid) return { layout: defaultLayout(), recovered: true };
    throw error;
  }
}
