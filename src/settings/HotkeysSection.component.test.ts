// Раздел «Горячие клавиши» (решение 72, Э12.5б–г): запись, конфликт, сброс,
// русская раскладка, системные сочетания.
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { HotkeyOverrides } from '../api/index.ts';
import HotkeysSection from './sections/HotkeysSection.svelte';

function setup(hotkeys: HotkeyOverrides = {}) {
  const onChange = vi.fn<(next: HotkeyOverrides) => void>();
  const view = render(HotkeysSection, { hotkeys, onChange });
  const row = (label: string) => {
    const item = screen.getAllByRole('listitem').find((li) => li.textContent?.includes(label));
    if (!item) throw new Error(`нет строки ${label}`);
    return within(item);
  };
  return { onChange, row, view };
}

const press = (init: KeyboardEventInit) => fireEvent.keyDown(window, init);

afterEach(cleanup);

describe('раздел «Горячие клавиши»', () => {
  it('команды реестра с сочетаниями и системные — с замком, без кнопок', () => {
    const { row } = setup();
    expect(row('Быстрый переход').getByRole('button', { name: 'Изменить сочетание Ctrl+O' })).toBeTruthy();
    expect(row('Новая заметка').getByRole('button', { name: 'Изменить сочетание Ctrl+N' })).toBeTruthy();
    expect(row('Переключить режим заметки').getByText('Живой предпросмотр / исходный текст')).toBeTruthy();
    expect(row('Назад по истории').getByRole('button', { name: 'Изменить сочетание Alt+←' })).toBeTruthy();
    expect(row('Переименовать').getByRole('button', { name: 'Изменить сочетание F2' })).toBeTruthy();
    expect(row('Удалить в корзину').getByRole('button', { name: 'Изменить сочетание Delete' })).toBeTruthy();

    const copy = row('Копировать, вырезать, вставить');
    expect(copy.getByText('системное')).toBeTruthy();
    expect(copy.getByTitle('Системное сочетание, его нельзя переназначить')).toBeTruthy();
    expect(copy.queryAllByRole('button')).toHaveLength(0);
  });

  it('клик по сочетанию — запись; новое сочетание по физической клавише (русская раскладка)', async () => {
    const { onChange, row } = setup();
    await fireEvent.click(row('Быстрый переход').getByRole('button', { name: 'Изменить сочетание Ctrl+O' }));
    expect(screen.getByTestId('hotkey-recorder').textContent).toContain('Нажмите сочетание…');
    expect(row('Быстрый переход').getByText('Идёт запись: нажмите новое сочетание вместо «Ctrl+O».')).toBeTruthy();

    // Только модификатор — запись продолжается.
    await press({ key: 'Control', code: 'ControlLeft', ctrlKey: true });
    expect(onChange).not.toHaveBeenCalled();
    await press({ key: 'з', code: 'KeyP', ctrlKey: true });

    expect(onChange).toHaveBeenCalledWith({ 'quick_switcher.open': ['Ctrl+KeyP'] });
    expect(screen.queryByTestId('hotkey-recorder')).toBeNull();
  });

  it('Esc отменяет запись, прежнее сочетание остаётся; клик мимо — тоже отмена', async () => {
    const { onChange, row } = setup();
    await fireEvent.click(row('Переименовать').getByRole('button', { name: 'Изменить сочетание F2' }));
    const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
    window.dispatchEvent(escape);
    await Promise.resolve();
    expect(screen.queryByTestId('hotkey-recorder')).toBeNull();
    expect(escape.defaultPrevented).toBe(true);

    await fireEvent.click(row('Переименовать').getByRole('button', { name: 'Изменить сочетание F2' }));
    await fireEvent.pointerDown(document.body);
    expect(screen.queryByTestId('hotkey-recorder')).toBeNull();
    expect(onChange).not.toHaveBeenCalled();
  });

  it('системное и без Ctrl/Alt — не записываются, с пояснением', async () => {
    const { onChange, row } = setup();
    await fireEvent.click(row('Переименовать').getByRole('button', { name: 'Изменить сочетание F2' }));

    await press({ key: 'с', code: 'KeyC', ctrlKey: true });
    expect(row('Переименовать').getByText('Это сочетание системное, его нельзя назначить.')).toBeTruthy();
    await press({ key: 'F4', code: 'F4', altKey: true });
    expect(row('Переименовать').getByText('Это сочетание системное, его нельзя назначить.')).toBeTruthy();
    await press({ key: 'к', code: 'KeyR' });
    expect(row('Переименовать').getByText('Нужна клавиша Ctrl или Alt; без них — только F1–F12 и Delete.')).toBeTruthy();
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByTestId('hotkey-recorder')).toBeTruthy();
  });

  it('конфликт: предупреждение; Enter и «Отмена» ничего не меняют, «Заменить» снимает у прежней команды', async () => {
    const { onChange, row } = setup();
    const startConflict = async () => {
      await fireEvent.click(row('Переименовать').getByRole('button', { name: 'Изменить сочетание F2' }));
      await press({ key: 'т', code: 'KeyN', ctrlKey: true });
    };

    await startConflict();
    const panel = screen.getByRole('alertdialog');
    expect(panel.textContent).toContain('«Ctrl+N» уже назначено команде «Новая заметка».');
    expect(row('Переименовать').getByText('Сочетание занято — выберите, что сделать.')).toBeTruthy();
    // Решение 73: Enter — «Отмена» (фокус на ней), Esc — тоже.
    expect(document.activeElement?.textContent?.trim()).toBe('Отмена');
    await press({ key: 'Enter', code: 'Enter' });
    expect(screen.queryByRole('alertdialog')).toBeNull();

    await startConflict();
    await press({ key: 'Escape', code: 'Escape' });
    expect(screen.queryByRole('alertdialog')).toBeNull();

    await startConflict();
    await fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Отмена' }));
    expect(onChange).not.toHaveBeenCalled();

    await startConflict();
    await fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Заменить' }));
    expect(onChange).toHaveBeenCalledWith({ 'note.new': [], 'tree.rename': ['Ctrl+KeyN'] });
  });

  it('«+» добавляет ещё одно сочетание; «×» убирает', async () => {
    const { onChange, row } = setup();
    await fireEvent.click(row('Быстрый переход').getByRole('button', { name: 'Добавить сочетание команде «Быстрый переход»' }));
    expect(row('Быстрый переход').getByText('Идёт запись: нажмите новое сочетание.')).toBeTruthy();
    await press({ key: 'з', code: 'KeyP', ctrlKey: true, shiftKey: true });
    expect(onChange).toHaveBeenLastCalledWith({ 'quick_switcher.open': ['Ctrl+KeyO', 'Ctrl+Shift+KeyP'] });

    await fireEvent.click(row('Удалить в корзину').getByRole('button', { name: 'Убрать сочетание Delete' }));
    expect(onChange).toHaveBeenLastCalledWith({ 'tree.delete': [] });
  });

  it('изменённая команда — «изменено» и «↺»; «Сбросить все» возвращает стандартные, чужие назначения остаются', async () => {
    const { onChange, row } = setup({
      'editor.toggle-mode': ['Ctrl+KeyE', 'Ctrl+Shift+KeyE'],
      'graph.open': ['Ctrl+KeyG'],
    });
    const mode = row('Переключить режим заметки');
    expect(mode.getByText('изменено')).toBeTruthy();
    expect(row('Быстрый переход').queryByText('изменено')).toBeNull();

    await fireEvent.click(mode.getByRole('button', { name: 'Вернуть стандартные сочетания команды «Переключить режим заметки»' }));
    expect(onChange).toHaveBeenLastCalledWith({ 'graph.open': ['Ctrl+KeyG'] });

    await fireEvent.click(screen.getByRole('button', { name: 'Сбросить все' }));
    expect(onChange).toHaveBeenLastCalledWith({ 'graph.open': ['Ctrl+KeyG'] });
  });

  it('поиск оставляет команды, в названии которых есть буквы', async () => {
    setup();
    await fireEvent.input(screen.getByRole('textbox', { name: 'Найти команду…' }), { target: { value: 'пере' } });
    const labels = screen.getAllByRole('listitem').map((li) => li.querySelector('.label span')?.textContent);
    expect(labels).toEqual(['Быстрый переход', 'Переключить режим заметки', 'Переименовать']);

    await fireEvent.input(screen.getByRole('textbox', { name: 'Найти команду…' }), { target: { value: 'ыыы' } });
    expect(screen.getByText('Команд не найдено.')).toBeTruthy();
  });
});
