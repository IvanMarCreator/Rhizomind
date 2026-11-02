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
});
