// Вид «Обратные ссылки» в окне (0.2-2.2, С2-3.2): заголовок панели с числом,
// щелчок по строке открывает заметку и ставит курсор на ссылку (фокус в
// текст, как в решении 86), щелчок по имени — заметка сверху.
import { EditorView } from '@codemirror/view';
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { Backlinks, NoteReadResponse, Settings, VaultPath } from '../api/index.ts';
import { indexMocks, mockCore } from '../api/testing.ts';
import Shell from '../shell/Shell.svelte';
import { WindowModel } from '../shell/window.svelte.ts';

// В jsdom нет раскладки: у Range нет размеров, а CodeMirror меряет текст при фокусе.
if (typeof Range.prototype.getClientRects !== 'function') {
  const empty = { x: 0, y: 0, width: 0, height: 0, top: 0, left: 0, right: 0, bottom: 0, toJSON: () => ({}) };
  Object.assign(Range.prototype, {
    getClientRects: () => Object.assign([], { item: () => null }) as unknown as DOMRectList,
    getBoundingClientRect: () => empty as DOMRect,
  });
}

const SETTINGS: Settings = {
  theme: 'system',
  editor_mode: 'source',
  text_width: 'column',
  font_size: 16,
  new_note_location: { kind: 'vault_root' },
  confirm_delete: true,
  max_edit_size_bytes: 10 * 1024 * 1024,
  hotkeys: {},
  rename_links: 'ask',
};

const VERSION = { hash: 'a'.repeat(64), size: 1, modified_ms: 1 };
const TARGET: VaultPath = ['Проекты', 'Встреча по запуску.md'];
const PLAN: VaultPath = ['План работ.md'];
const LINK = '[[Встреча по запуску]]';
const PLAN_LINE = `Срок пилота — «пилот» 😀 согласовали на ${LINK}.`;
const PLAN_TEXT = `# План работ\n\n${'строка\n'.repeat(40)}${PLAN_LINE}\nконец\n`;
const PLAN_LINE_NO = 43;

const BACKLINKS: Backlinks = {
  notes_total: 1,
  truncated: false,
  notes: [
    {
      from: PLAN,
      name: 'План работ',
      links_total: 1,
      links: [
        {
          line: PLAN_LINE_NO,
          column_utf16: PLAN_LINE.indexOf(LINK),
          snippet: PLAN_LINE,
          from_utf16: PLAN_LINE.indexOf(LINK),
          to_utf16: PLAN_LINE.indexOf(LINK) + LINK.length,
        },
      ],
    },
  ],
};

async function openTarget() {
  const files = new Map([
    [TARGET.join('/'), '# Встреча по запуску\n'],
    [PLAN.join('/'), PLAN_TEXT],
  ]);
  mockCore({
    ...indexMocks({ backlinks: { [TARGET.join('/')]: BACKLINKS } }),
    vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'База' } }),
    settings_read: () => ({ settings: { ...SETTINGS } }),
    settings_write: () => null,
    workspace_read: () => ({
      workspace: {
        format: 'rhizomind-layout',
        version: 1,
        root: { type: 'pane', id: 'pane-1', tabs: [{ type: 'note', path: TARGET }], active: 0 },
        active_pane: 'pane-1',
        side_panel: { open: true, view: 'backlinks', width: 300, tag_order: 'name' },
      },
    }),
    workspace_write: () => null,
    tree_list: () => ({ entries: [] }),
    note_read: ({ path }): NoteReadResponse => ({ text: files.get(path.join('/')) ?? '', version: VERSION, read_only: null }),
    note_write: () => ({ outcome: 'saved', version: VERSION }),
    links_resolve: ({ targets }) => ({ results: targets.map((target) => ({ target, path: null })) }),
  });
  const model = new WindowModel();
  render(Shell, { model });
  const view = await editorView();
  const panel = await screen.findByRole('complementary', { name: 'Правая панель' });
  await waitFor(() => expect(within(panel).getAllByRole('button', { name: /^Открыть/ })).toHaveLength(1));
  return { model, view, panel };
}

async function editorView(): Promise<EditorView> {
  const content = await screen.findByRole('textbox', { name: 'Текст заметки' });
  const view = EditorView.findFromDOM(content);
  if (!view) throw new Error('нет редактора');
  return view;
}

async function nextView(previous: EditorView): Promise<EditorView> {
  let view = previous;
  await waitFor(async () => {
    view = await editorView();
    expect(view).not.toBe(previous);
  });
  return view;
}

afterEach(() => {
  cleanup();
  clearMocks();
});

describe('обратные ссылки в окне', () => {
  it('заголовок с числом заметок', async () => {
    const { panel } = await openTarget();
    const heading = within(panel).getByRole('heading', { level: 2 });
    expect(heading.textContent?.replace(/\s+/g, ' ').trim()).toBe('Обратные ссылки 1');
  });

  it('щелчок по строке открывает заметку и ставит курсор на ссылку, фокус в текст', async () => {
    const { model, view, panel } = await openTarget();

    await fireEvent.click(within(panel).getByRole('button', { name: new RegExp(`^${PLAN_LINE.slice(0, 10)}`) }));

    await waitFor(() => expect(model.currentPath).toEqual(PLAN));
    const plan = await nextView(view);
    await waitFor(() => {
      const head = plan.state.selection.main.head;
      const line = plan.state.doc.lineAt(head);
      expect([line.number, head - line.from]).toEqual([PLAN_LINE_NO, PLAN_LINE.indexOf(LINK)]);
    });
    expect(plan.contentDOM.contains(document.activeElement)).toBe(true);
    // Переход — шаг истории.
    expect(model.canGoBack).toBe(true);
  });

  it('щелчок по имени — заметка сверху', async () => {
    const { model, view, panel } = await openTarget();

    await fireEvent.click(within(panel).getByRole('button', { name: 'Открыть «План работ»' }));

    await waitFor(() => expect(model.currentPath).toEqual(PLAN));
    const plan = await nextView(view);
    await waitFor(() => expect(plan.contentDOM.contains(document.activeElement)).toBe(true));
    expect(plan.state.selection.main.head).toBe(0);
  });
});
