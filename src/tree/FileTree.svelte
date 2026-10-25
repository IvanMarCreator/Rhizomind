<!--
  Дерево файлов (С3.2, макет К2 вариант 3Б: без значков, метки типа у
  вложений). Все папки и файлы хранилища, папки сверху; кнопки «Новая
  заметка» и «Новая папка» при наведении на папку или на пустое место;
  контекстное меню; F2 и двойной клик — переименование; перетаскивание —
  перемещение; Delete — в корзину. Обновляется по событию `vault:changed`.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import type { TreeEntry, VaultPath } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import ConfirmDelete from './ConfirmDelete.svelte';
  import NameField from './NameField.svelte';
  import TreeIcon from './TreeIcon.svelte';
  import TreeMenu, { type MenuItem } from './TreeMenu.svelte';
  import { canMove, displayName, parentOf, pathKey, samePath, splitName, typeLabel, type TreeRow } from './model.ts';
  import type { FileTreeState } from './tree.svelte.ts';

  let { tree }: { tree: FileTreeState } = $props();

  /** Перетаскиваемый элемент; данные в DataTransfer не кладутся: путь знает только дерево. */
  const DRAG_TYPE = 'application/x-rhizomind-entry';

  let treeElement: HTMLDivElement | undefined = $state();
  /** Строка под указателем: ключ пути; `''` — пустое место дерева (корень). */
  let hovered = $state<string | null>(null);
  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
  let dragging = $state.raw<VaultPath | null>(null);
  /** Папка, куда упадёт перетаскиваемый элемент (ключ пути; `''` — корень). */
  let dropTarget = $state<string | null>(null);

  /** Постоянные id строк для aria-activedescendant (кэш, не состояние). */
  // Без прототипа: имя файла «constructor» не должно найти свойство Object.
  const rowIds: Record<string, string> = Object.create(null) as Record<string, string>;
  let nextRowId = 1;

  onMount(() => tree.start());

  const selectedIndex = $derived(
    tree.selected ? tree.rows.findIndex((row) => samePath(row.node.path, tree.selected as VaultPath)) : -1,
  );
  const empty = $derived(tree.loaded && tree.rows.length === 0);

  function rowId(path: VaultPath): string {
    const key = pathKey(path);
    rowIds[key] ??= `tree-row-${nextRowId++}`;
    return rowIds[key];
  }

  function focusTree() {
    treeElement?.focus();
  }

  function isCurrent(path: VaultPath): boolean {
    return tree.host.currentPath !== null && samePath(tree.host.currentPath, path);
  }

  // ---- Выбор и клавиши -------------------------------------------------------

  function selectIndex(index: number) {
    const row = tree.rows[Math.max(0, Math.min(index, tree.rows.length - 1))];
    if (!row) return;
    tree.select(row.node.path);
    document.getElementById(rowId(row.node.path))?.scrollIntoView?.({ block: 'nearest' });
  }

  function onTreeKeydown(event: KeyboardEvent) {
    // Клавиши поля имени и меню сюда не относятся.
    if (event.target !== treeElement) return;
    // Alt+←/→ — история переходов окна, Ctrl+… — общие сочетания: не дерево.
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    const row = selectedIndex >= 0 ? tree.rows[selectedIndex] : undefined;
    const handled = handleKey(event, row);
    if (handled) event.preventDefault();
  }

  function handleKey(event: KeyboardEvent, row: TreeRow | undefined): boolean {
    switch (event.key) {
      case 'ArrowDown':
        selectIndex(selectedIndex + 1);
        return true;
      case 'ArrowUp':
        selectIndex(selectedIndex < 0 ? 0 : selectedIndex - 1);
        return true;
      case 'Home':
        selectIndex(0);
        return true;
      case 'End':
        selectIndex(tree.rows.length - 1);
        return true;
      case 'ArrowRight':
        if (row?.node.kind === 'folder') {
          if (!tree.isExpanded(row.node.path)) tree.setExpanded(row.node.path, true);
          else if (row.node.children.length > 0) selectIndex(selectedIndex + 1);
        }
        return true;
      case 'ArrowLeft':
        if (!row) return true;
        if (row.node.kind === 'folder' && tree.isExpanded(row.node.path)) tree.setExpanded(row.node.path, false);
        else if (row.node.path.length > 1) tree.select(parentOf(row.node.path));
        return true;
      case 'Enter':
        if (row) tree.activate(row.node.path);
        return true;
      case 'F2':
        if (row) tree.startRename(row.node.path);
        return true;
      case 'Delete':
        if (row) tree.requestDelete(row.node.path);
        return true;
      case 'ContextMenu':
        openMenuForSelected();
        return true;
      case 'F10':
        if (!event.shiftKey) return false;
        openMenuForSelected();
        return true;
      default:
        return false;
    }
  }

  function openMenuForSelected() {
    const row = selectedIndex >= 0 ? tree.rows[selectedIndex] : undefined;
    const element = row ? document.getElementById(rowId(row.node.path)) : treeElement;
    const box = element?.getBoundingClientRect();
    openMenu(row?.node ?? null, (box?.left ?? 0) + 24, box?.bottom ?? 0);
  }

  // ---- Мышь ----------------------------------------------------------------

  function onRowClick(row: TreeRow) {
    focusTree();
    tree.activate(row.node.path);
  }

  function onRowDblClick(row: TreeRow) {
    // Заметка открылась первым кликом, папка раскрылась и закрылась: имя правится (Э4).
    if (row.node.kind === 'folder') tree.toggle(row.node.path);
    tree.startRename(row.node.path);
  }

  function onRowContextMenu(event: MouseEvent, row: TreeRow) {
    event.preventDefault();
    event.stopPropagation();
    tree.select(row.node.path);
    openMenu(row.node, event.clientX, event.clientY);
  }

  function onRootContextMenu(event: MouseEvent) {
    event.preventDefault();
    openMenu(null, event.clientX, event.clientY);
  }

  function openMenu(entry: TreeEntry | null, x: number, y: number) {
    const items: MenuItem[] = [];
    if (!entry || entry.kind === 'folder') {
      const dir = entry?.path ?? [];
      items.push(
        { id: 'new-note', label: t('tree.newNote'), run: () => void tree.createNote(dir) },
        { id: 'new-folder', label: t('tree.newFolder'), run: () => void tree.createFolder(dir) },
      );
    }
    if (entry) {
      const path = entry.path;
      items.push(
        { id: 'rename', label: t('tree.rename'), shortcut: t('tree.key.rename'), run: () => tree.startRename(path) },
        { id: 'delete', label: t('tree.delete'), shortcut: t('tree.key.delete'), run: () => tree.requestDelete(path) },
      );
    }
    menu = { x, y, items };
  }

  function closeMenu() {
    menu = null;
    focusTree();
  }

  function create(kind: 'note' | 'folder', dir: VaultPath, event: MouseEvent) {
    event.stopPropagation();
    void (kind === 'note' ? tree.createNote(dir) : tree.createFolder(dir));
  }

  // ---- Перетаскивание --------------------------------------------------------

  /** Куда падает элемент над строкой: на папку — в неё, на файл — в папку файла. */
  function dropDir(row: TreeRow | null): VaultPath {
    if (!row) return [];
    return row.node.kind === 'folder' ? row.node.path : parentOf(row.node.path);
  }

  function onDragStart(event: DragEvent, row: TreeRow) {
    if (tree.renaming) {
      event.preventDefault();
      return;
    }
    dragging = row.node.path;
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'move';
      event.dataTransfer.setData(DRAG_TYPE, '');
    }
  }

  function onDragOver(event: DragEvent, row: TreeRow | null) {
    if (!dragging) return;
    event.stopPropagation();
    const dir = dropDir(row);
    if (!canMove(dragging, dir)) {
      dropTarget = null;
      return;
    }
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
    dropTarget = pathKey(dir);
  }

  function onDrop(event: DragEvent, row: TreeRow | null) {
    if (!dragging) return;
    event.preventDefault();
    event.stopPropagation();
    const path = dragging;
    const dir = dropDir(row);
    dragging = null;
    dropTarget = null;
    void tree.move(path, dir);
  }

  function onDragEnd() {
    dragging = null;
    dropTarget = null;
  }

  function guides(level: number): number[] {
    return Array.from({ length: level - 1 }, (_, i) => i);
  }
</script>

<div class="file-tree">
  <div
    bind:this={treeElement}
    class="rows"
    role="tree"
    tabindex="0"
    aria-label={t('tree.label')}
    aria-activedescendant={selectedIndex >= 0 ? rowId(tree.rows[selectedIndex]?.node.path ?? []) : undefined}
    onkeydown={onTreeKeydown}
  >
    {#each tree.rows as row (pathKey(row.node.path))}
      {@const key = pathKey(row.node.path)}
      {@const folder = row.node.kind === 'folder'}
      {@const editing = tree.renaming !== null && samePath(tree.renaming, row.node.path)}
      {@const label = typeLabel(row.node)}
      <div
        id={rowId(row.node.path)}
        class="row"
        class:folder
        class:attachment={row.node.kind === 'attachment'}
        class:selected={selectedIndex >= 0 && tree.rows[selectedIndex] === row}
        class:current={isCurrent(row.node.path)}
        class:drop-target={dropTarget === key && folder}
        class:editing
        role="treeitem"
        aria-level={row.level}
        aria-label={displayName(row.node)}
        aria-expanded={folder ? tree.isExpanded(row.node.path) : undefined}
        aria-selected={tree.selected !== null && samePath(tree.selected, row.node.path)}
        aria-current={isCurrent(row.node.path) ? 'page' : undefined}
        title={row.node.kind === 'attachment' ? t('tree.notOpenable') : undefined}
        tabindex="-1"
        draggable={!editing}
        style:--level={row.level}
        onclick={() => onRowClick(row)}
        ondblclick={() => onRowDblClick(row)}
        onkeydown={() => {}}
        oncontextmenu={(event) => onRowContextMenu(event, row)}
        onmouseenter={() => (hovered = key)}
        onmouseleave={() => (hovered = hovered === key ? null : hovered)}
        ondragstart={(event) => onDragStart(event, row)}
        ondragenter={(event) => onDragOver(event, row)}
        ondragover={(event) => onDragOver(event, row)}
        ondrop={(event) => onDrop(event, row)}
        ondragend={onDragEnd}
      >
        {#each guides(row.level) as i (i)}
          <span class="guide" style:--guide={i} aria-hidden="true"></span>
        {/each}
        <span class="chevron" class:open={folder && tree.isExpanded(row.node.path)} aria-hidden="true">
          {#if folder}<TreeIcon name="chevron-right" size={14} />{/if}
        </span>
        {#if editing}
          {@const parts = splitName(row.node)}
          <NameField
            dir={parentOf(row.node.path)}
            stem={parts.stem}
            ext={parts.ext}
            renaming={row.node.path}
            submit={(name) => tree.rename(row.node.path, name)}
            onDone={() => {
              tree.cancelRename();
              focusTree();
            }}
          />
        {:else}
          <span class="name">{displayName(row.node)}</span>
          {#if label}<span class="type">{label}</span>{/if}
          {#if folder && hovered === key}
            <span class="actions">
              <button
                type="button"
                class="action"
                aria-label={t('tree.newNote')}
                title={t('tree.newNote')}
                onclick={(event) => create('note', row.node.path, event)}
              >
                <TreeIcon name="square-pen" />
              </button>
              <button
                type="button"
                class="action"
                aria-label={t('tree.newFolder')}
                title={t('tree.newFolder')}
                onclick={(event) => create('folder', row.node.path, event)}
              >
                <TreeIcon name="folder-plus" />
              </button>
            </span>
          {/if}
        {/if}
      </div>
    {/each}
  </div>

  <div
    class="root-zone"
    class:drop-target={dropTarget === ''}
    data-testid="tree-root-zone"
    role="presentation"
    onmouseenter={() => (hovered = '')}
    onmouseleave={() => (hovered = hovered === '' ? null : hovered)}
    oncontextmenu={onRootContextMenu}
    ondragenter={(event) => onDragOver(event, null)}
    ondragover={(event) => onDragOver(event, null)}
    ondrop={(event) => onDrop(event, null)}
  >
    {#if hovered === ''}
      <span class="actions root-actions">
        <button
          type="button"
          class="action"
          aria-label={t('tree.newNote')}
          title={t('tree.newNote')}
          onclick={(event) => create('note', [], event)}
        >
          <TreeIcon name="square-pen" />
        </button>
        <button
          type="button"
          class="action"
          aria-label={t('tree.newFolder')}
          title={t('tree.newFolder')}
          onclick={(event) => create('folder', [], event)}
        >
          <TreeIcon name="folder-plus" />
        </button>
      </span>
    {/if}
    {#if empty}
      <p class="empty">{t('tree.empty')}</p>
    {/if}
  </div>
</div>

{#if menu}
  <TreeMenu x={menu.x} y={menu.y} items={menu.items} onClose={closeMenu} />
{/if}

{#if tree.pendingDelete}
  <ConfirmDelete
    entry={tree.pendingDelete}
    onConfirm={() => {
      void tree.confirmPendingDelete();
      focusTree();
    }}
    onCancel={() => {
      tree.cancelDelete();
      focusTree();
    }}
  />
{/if}

<style>
  .file-tree {
    display: flex;
    flex-direction: column;
    min-height: 100%;
    padding: 2px 6px 0;
    box-sizing: border-box;
  }

  .rows {
    outline: none;
  }

  .rows:focus-visible .row.selected {
    box-shadow: inset 0 0 0 1px var(--accent);
  }

  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 2px;
    min-height: 26px;
    padding: 0 4px 0 calc(6px + (var(--level) - 1) * 18px);
    border-radius: 4px;
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .row.folder {
    color: var(--text-primary);
  }

  .row.attachment {
    color: var(--text-faint);
  }

  .row:hover {
    background: var(--bg-hover);
  }

  .row.current {
    background: var(--bg-active);
    color: var(--text-primary);
    font-weight: 500;
  }

  .row.drop-target,
  .root-zone.drop-target {
    background: var(--accent-subtle);
    box-shadow: inset 0 0 0 1px var(--accent);
  }

  .row.editing {
    align-items: flex-start;
    padding-top: 3px;
    padding-bottom: 3px;
    background: none;
    white-space: normal;
  }

  .guide {
    position: absolute;
    top: 0;
    bottom: 0;
    left: calc(6px + var(--guide) * 18px + 8px);
    width: 1px;
    background: var(--border-default);
  }

  .chevron {
    display: grid;
    place-items: center;
    flex: none;
    width: 16px;
    height: 20px;
    color: var(--icon-muted);
    transition: transform 0.1s ease;
  }

  .chevron.open {
    transform: rotate(90deg);
  }

  .name {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .type {
    flex: none;
    height: 16px;
    margin-left: 6px;
    padding: 0 4px;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    color: var(--text-faint);
    font-size: 9px;
    font-weight: 600;
    line-height: 14px;
    box-sizing: border-box;
  }

  .actions {
    display: flex;
    gap: 2px;
    margin-left: auto;
    padding-left: 6px;
  }

  .action {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 4px;
    color: var(--icon-default);
  }

  .action:hover {
    background: var(--bg-active);
  }

  .root-zone {
    position: relative;
    flex: 1;
    min-height: 34px;
    border-radius: 4px;
  }

  .root-actions {
    position: absolute;
    top: 4px;
    right: 4px;
  }

  .empty {
    margin: 8px 6px;
    color: var(--text-muted);
    font-size: var(--font-size-caption);
  }
</style>
