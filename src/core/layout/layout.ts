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

export interface Layout {
  version: typeof LAYOUT_VERSION;
  root: LayoutNode;
  activePane: string;
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

export function defaultLayout(): Layout {
  return { version: LAYOUT_VERSION, root: { type: 'pane', id: 'pane-1', tabs: [], active: -1 }, activePane: 'pane-1' };
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

/** Объект для `workspace_write`; больше 1 МБ в JSON — `LayoutTooLarge`. */
export function serializeLayout(layout: Layout): Workspace {
  const workspace: Workspace = {
    format: LAYOUT_FORMAT,
    version: layout.version,
    root: layout.root,
    active_pane: layout.activePane,
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
    return { layout: { version: LAYOUT_VERSION, root, activePane: active }, recovered: false };
  } catch (error) {
    if (error instanceof Invalid) return { layout: defaultLayout(), recovered: true };
    throw error;
  }
}
