import { describe, expect, it } from 'vitest';
import {
  COMMANDS,
  assignChord,
  bindingsOf,
  checkChord,
  chordFromEvent,
  chordParts,
  chordText,
  isChanged,
  matchCommand,
  ownerOf,
  removeChord,
  resetCommand,
  type KeyLike,
} from './hotkeys.ts';

const key = (init: Partial<KeyLike> & { code: string }): KeyLike => ({
  key: init.key ?? '',
  code: init.code,
  ctrlKey: init.ctrlKey ?? false,
  altKey: init.altKey ?? false,
  shiftKey: init.shiftKey ?? false,
  metaKey: init.metaKey ?? false,
  isComposing: init.isComposing ?? false,
});

describe('реестр команд', () => {
  it('все сочетания интерфейса 0.1 в одном реестре, без повторов', () => {
    const defaults = Object.fromEntries(COMMANDS.map((c) => [c.id, c.defaults]));
    expect(defaults).toEqual({
      'quick_switcher.open': ['Ctrl+KeyO'],
      'note.new': ['Ctrl+KeyN'],
      'editor.toggle-mode': ['Ctrl+KeyE'],
      'history.back': ['Alt+ArrowLeft'],
      'history.forward': ['Alt+ArrowRight'],
      'tree.rename': ['F2'],
      'tree.delete': ['Delete'],
    });
    const all = COMMANDS.flatMap((c) => c.defaults);
    expect(new Set(all).size).toBe(all.length);
    for (const chord of all) expect(checkChord(chord)).toBeNull();
  });
});

describe('сочетание из нажатия', () => {
  it('по физической клавише: Ctrl+O в русской раскладке — Ctrl+KeyO', () => {
    expect(chordFromEvent(key({ key: 'щ', code: 'KeyO', ctrlKey: true }))).toBe('Ctrl+KeyO');
    expect(chordFromEvent(key({ key: 'O', code: 'KeyO', ctrlKey: true, shiftKey: true }))).toBe('Ctrl+Shift+KeyO');
    expect(chordFromEvent(key({ key: 'ArrowLeft', code: 'ArrowLeft', altKey: true }))).toBe('Alt+ArrowLeft');
    expect(chordFromEvent(key({ key: 'F2', code: 'F2' }))).toBe('F2');
  });

  it('без code (ввод через службу текста) — по латинской букве из key', () => {
    expect(chordFromEvent(key({ key: 'o', code: '', ctrlKey: true }))).toBe('Ctrl+KeyO');
    expect(chordFromEvent(key({ key: 'F2', code: '' }))).toBe('F2');
  });

  it('Cmd на macOS считается Ctrl', () => {
    expect(chordFromEvent(key({ key: 'o', code: 'KeyO', metaKey: true }))).toBe('Ctrl+KeyO');
  });

  it('одни модификаторы, неизвестная клавиша и набор через IME — нет сочетания', () => {
    expect(chordFromEvent(key({ key: 'Control', code: 'ControlLeft', ctrlKey: true }))).toBeNull();
    expect(chordFromEvent(key({ key: 'Escape', code: 'Escape' }))).toBeNull();
    expect(chordFromEvent(key({ key: 'o', code: 'KeyO', ctrlKey: true, isComposing: true }))).toBeNull();
  });
});

describe('проверка сочетания', () => {
  it('системные не назначаются', () => {
    expect(checkChord('Alt+F4')).toBe('reserved');
    expect(checkChord('Ctrl+KeyC')).toBe('reserved');
    expect(checkChord('Ctrl+KeyZ')).toBe('reserved');
  });

  it('буква без Ctrl или Alt мешала бы вводу', () => {
    expect(checkChord('KeyN')).toBe('needs_modifier');
    expect(checkChord('Shift+KeyN')).toBe('needs_modifier');
    expect(checkChord('ArrowLeft')).toBe('needs_modifier');
    expect(checkChord('F5')).toBeNull();
    expect(checkChord('Shift+Delete')).toBeNull();
  });
});

describe('назначения', () => {
  it('по умолчанию — сочетания реестра; назначение заменяет и добавляет', () => {
    expect(bindingsOf({}, 'quick_switcher.open')).toEqual(['Ctrl+KeyO']);
    const replaced = assignChord({}, 'quick_switcher.open', 'Ctrl+KeyP', 'Ctrl+KeyO');
    expect(replaced).toEqual({ 'quick_switcher.open': ['Ctrl+KeyP'] });
    const added = assignChord(replaced, 'quick_switcher.open', 'Ctrl+KeyO', null);
    expect(bindingsOf(added, 'quick_switcher.open')).toEqual(['Ctrl+KeyP', 'Ctrl+KeyO']);
    expect(isChanged(added, 'quick_switcher.open')).toBe(true);
  });

  it('конфликт: владелец сочетания; «заменить» снимает его у прежней команды', () => {
    expect(ownerOf({}, 'Ctrl+KeyN')?.id).toBe('note.new');
    expect(ownerOf({}, 'Ctrl+KeyN', 'note.new')).toBeNull();
    const next = assignChord({}, 'tree.rename', 'Ctrl+KeyN', 'F2');
    expect(bindingsOf(next, 'tree.rename')).toEqual(['Ctrl+KeyN']);
    expect(bindingsOf(next, 'note.new')).toEqual([]);
    expect(isChanged(next, 'note.new')).toBe(true);
  });

  it('сброс команды и возврат к стандартному без пометки «изменено»', () => {
    const changed = assignChord({}, 'tree.rename', 'F3', 'F2');
    expect(resetCommand(changed, 'tree.rename')).toEqual({});
    // Вернули то же, что по умолчанию, — назначение не хранится.
    expect(assignChord(changed, 'tree.rename', 'F2', 'F3')).toEqual({});
  });

  it('убрать сочетание; неизвестные команды (из новой версии) сохраняются', () => {
    const next = removeChord({ 'graph.open': ['Ctrl+KeyG'] }, 'tree.delete', 'Delete');
    expect(next).toEqual({ 'graph.open': ['Ctrl+KeyG'], 'tree.delete': [] });
  });
});

describe('распознавание команды', () => {
  it('по области: F2 — только в дереве, Ctrl+O — везде', () => {
    expect(matchCommand({}, key({ key: 'F2', code: 'F2' }), 'tree')).toBe('tree.rename');
    expect(matchCommand({}, key({ key: 'F2', code: 'F2' }), 'window')).toBeNull();
    expect(matchCommand({}, key({ key: 'щ', code: 'KeyO', ctrlKey: true }), 'window')).toBe('quick_switcher.open');
    expect(matchCommand({}, key({ key: 'ArrowLeft', code: 'ArrowLeft', altKey: true }), 'window')).toBe('history.back');
  });

  it('переназначенное сочетание работает, старое — нет', () => {
    const overrides = { 'quick_switcher.open': ['Ctrl+KeyP'] };
    expect(matchCommand(overrides, key({ key: 'з', code: 'KeyP', ctrlKey: true }), 'window')).toBe('quick_switcher.open');
    expect(matchCommand(overrides, key({ key: 'o', code: 'KeyO', ctrlKey: true }), 'window')).toBeNull();
  });
});

describe('показ сочетания', () => {
  it('клавиши по частям и строкой', () => {
    expect(chordParts('Ctrl+Shift+KeyE')).toEqual(['Ctrl', 'Shift', 'E']);
    expect(chordParts('Alt+ArrowLeft')).toEqual(['Alt', '←']);
    expect(chordText('Ctrl+KeyO')).toBe('Ctrl+O');
  });
});
