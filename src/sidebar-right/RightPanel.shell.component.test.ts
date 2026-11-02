// Правая панель в окне (0.2-2.1, С2-3.1): кнопка справа в строке над
// заметкой, команды реестра «Обратные ссылки» / «Теги», запись в workspace.
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { HotkeyOverrides, Settings, Workspace } from '../api/index.ts';
import { mockCore, type CoreMocks } from '../api/testing.ts';
import Shell from '../shell/Shell.svelte';
import { WindowModel } from '../shell/window.svelte.ts';

function settings(hotkeys: HotkeyOverrides = {}): Settings {
  return {
    theme: 'system',
    editor_mode: 'live_preview',
    text_width: 'column',
    font_size: 16,
    new_note_location: { kind: 'vault_root' },
    confirm_delete: true,
    max_edit_size_bytes: 10 * 1024 * 1024,
    hotkeys,
    rename_links: 'ask',
  };
}

function mocks(workspace: Workspace | null, hotkeys: HotkeyOverrides = {}): CoreMocks {
  return {
    vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'Рабочая база' } }),
    settings_read: () => ({ settings: settings(hotkeys) }),
    workspace_read: () => ({ workspace }),
    workspace_write: () => null,
    tree_list: () => ({ entries: [] }),
  };
}

const V01 = {
  format: 'rhizomind-layout',
  version: 1,
  root: { type: 'pane', id: 'pane-1', tabs: [], active: -1 },
  active_pane: 'pane-1',
};

function lastSidePanel(calls: [string, unknown][]): unknown {
  const writes = calls.filter(([command]) => command === 'workspace_write');
  const request = writes.at(-1)?.[1] as { request: { workspace: { side_panel?: unknown } } } | undefined;
  return request?.request.workspace.side_panel;
}

afterEach(() => {
  cleanup();
  clearMocks();
});

describe('правая панель в окне', () => {
  it('панель открывается кнопкой справа в строке над заметкой; кнопка нажата, пока панель открыта', async () => {
    const calls = mockCore(mocks(V01));
    render(Shell, { model: new WindowModel() });
    const button = await screen.findByRole('button', { name: 'Правая панель' });
    expect(button.getAttribute('aria-pressed')).toBe('false');
    expect(screen.queryByRole('complementary', { name: 'Правая панель' })).toBeNull();

    await fireEvent.click(button);

    expect(button.getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByRole('complementary', { name: 'Правая панель' })).toBeTruthy();
    await waitFor(() =>
      expect(lastSidePanel(calls)).toEqual({ open: true, view: 'backlinks', width: 300, tag_order: 'name' }),
    );

    await fireEvent.click(button);
    expect(screen.queryByRole('complementary', { name: 'Правая панель' })).toBeNull();
  });

  it('панель открывается командой: назначенное сочетание «Теги» открывает вид, повтор закрывает', async () => {
    mockCore(mocks(V01, { 'panel.tags': ['Ctrl+Shift+KeyT'] }));
    render(Shell, { model: new WindowModel() });
    await screen.findByRole('button', { name: 'Правая панель' });

    await fireEvent.keyDown(window, { key: 'T', code: 'KeyT', ctrlKey: true, shiftKey: true });
    const aside = screen.getByRole('complementary', { name: 'Правая панель' });
    expect(screen.getByRole('tab', { name: 'Теги' }).getAttribute('aria-selected')).toBe('true');
    expect(aside).toBeTruthy();

    await fireEvent.keyDown(window, { key: 'T', code: 'KeyT', ctrlKey: true, shiftKey: true });
    expect(screen.queryByRole('complementary', { name: 'Правая панель' })).toBeNull();
  });

  it('состояние восстанавливается из workspace: открыта, вид, ширина', async () => {
    mockCore(mocks({ ...V01, side_panel: { open: true, view: 'tags', width: 420, tag_order: 'count' } }));
    render(Shell, { model: new WindowModel() });

    const aside = await screen.findByRole('complementary', { name: 'Правая панель' });
    expect(aside.style.width).toBe('420px');
    expect(screen.getByRole('tab', { name: 'Теги' }).getAttribute('aria-selected')).toBe('true');
    expect(screen.getByRole('button', { name: 'Правая панель' }).getAttribute('aria-pressed')).toBe('true');
  });

  it('workspace 0.1 без поля: окно открывается без уведомления о сбое, панель закрыта', async () => {
    mockCore(mocks(V01));
    render(Shell, { model: new WindowModel() });

    await screen.findByText('Нет открытой заметки');
    expect(screen.queryByRole('complementary', { name: 'Правая панель' })).toBeNull();
    expect(screen.queryByText(/Раскладка/)).toBeNull();
  });

  it('записи раскладки идут по очереди: последним записывается последнее состояние', async () => {
    const written: unknown[] = [];
    let release: (() => void) | undefined;
    mockCore({
      ...mocks(V01),
      workspace_write: ({ workspace }) => {
        written.push((workspace as { side_panel?: unknown }).side_panel);
        // Первая запись «висит», пока её не отпустят.
        if (written.length === 1) return new Promise<null>((resolve) => (release = () => resolve(null)));
        return null;
      },
    });
    render(Shell, { model: new WindowModel() });
    const button = await screen.findByRole('button', { name: 'Правая панель' });

    await fireEvent.click(button); // открыта — запись 1 висит
    await fireEvent.click(screen.getByRole('tab', { name: 'Теги' })); // ждёт
    await fireEvent.click(button); // закрыта — ждёт
    expect(written).toHaveLength(1);

    release?.();
    await waitFor(() => expect(written).toHaveLength(2));
    // Промежуточный снимок не пишется: следующая запись берёт последнее состояние.
    expect(written).toEqual([
      { open: true, view: 'backlinks', width: 300, tag_order: 'name' },
      { open: false, view: 'tags', width: 300, tag_order: 'name' },
    ]);
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(written).toHaveLength(2);
  });

  it('запись правой панели — через WindowModel: панель открыта методом окна', async () => {
    const calls = mockCore(mocks(V01));
    const model = new WindowModel();
    render(Shell, { model });
    await screen.findByRole('button', { name: 'Правая панель' });

    await model.setRightPanel({ ...model.layout.rightPanel, open: true, view: 'tags' }, true);

    expect(screen.getByRole('tab', { name: 'Теги' }).getAttribute('aria-selected')).toBe('true');
    expect(lastSidePanel(calls)).toEqual({ open: true, view: 'tags', width: 300, tag_order: 'name' });
  });

  it('при закрытии панели командой фокус возвращается в текст заметки', async () => {
    mockCore({
      ...mocks(
        {
          ...V01,
          root: { type: 'pane', id: 'pane-1', tabs: [{ type: 'note', path: ['А.md'] }], active: 0 },
          side_panel: { open: true, view: 'backlinks', width: 300, tag_order: 'name' },
        },
        { 'panel.backlinks': ['Ctrl+Shift+KeyB'] },
      ),
      note_read: () => ({ text: 'текст', version: { hash: 'a'.repeat(64), size: 1, modified_ms: 1 }, read_only: null }),
      links_resolve: ({ targets }) => ({ results: targets.map((target) => ({ target, path: null })) }),
      backlinks_list: () => ({ status: 'ready', notes_total: 0, truncated: false, notes: [] }),
    });
    render(Shell, { model: new WindowModel() });
    const text = await screen.findByRole('textbox', { name: 'Текст заметки' });
    const tab = screen.getByRole('tab', { name: 'Обратные ссылки' });
    tab.focus();
    expect(document.activeElement).toBe(tab);

    await fireEvent.keyDown(tab, { key: 'B', code: 'KeyB', ctrlKey: true, shiftKey: true });

    expect(screen.queryByRole('complementary', { name: 'Правая панель' })).toBeNull();
    await waitFor(() => expect(text.contains(document.activeElement)).toBe(true));
  });
});

