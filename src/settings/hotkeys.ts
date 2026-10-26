// Горячие клавиши (решение владельца 72): единый реестр команд интерфейса,
// их сочетания по умолчанию и назначения пользователя поверх них
// (настройка хранилища `hotkeys`, проверка формата — в ядре,
// src-tauri/src/commands/hotkeys.rs; списки клавиш — LIMITS, общий снимок
// contracts/v1/limits.snap).
//
// Сочетание — строка `Ctrl+Alt+Shift+<code>`: модификаторы в этом порядке и
// физическая клавиша `KeyboardEvent.code`, поэтому сочетание работает в
// любой раскладке (в русской Ctrl+O приходит с key «щ», code — «KeyO»).
import type { HotkeyOverrides } from '../api/index.ts';
import { LIMITS } from '../api/limits.ts';
import type { MessageKey } from '../i18n/index.ts';

/**
 * Где действует команда: `window` — во всём окне, `tree` — когда фокус на
 * дереве файлов. Сочетание принадлежит одной команде в любой области.
 */
export type HotkeyScope = 'window' | 'tree';

export type CommandId =
  | 'quick_switcher.open'
  | 'note.new'
  | 'editor.toggle-mode'
  | 'history.back'
  | 'history.forward'
  | 'tree.rename'
  | 'tree.delete';

export interface HotkeyCommand {
  id: CommandId;
  scope: HotkeyScope;
  label: MessageKey;
  /** Пояснение под названием. */
  hint?: MessageKey;
  defaults: readonly string[];
}

/** Команды в порядке показа в настройках. */
export const COMMANDS: readonly HotkeyCommand[] = [
  { id: 'quick_switcher.open', scope: 'window', label: 'hotkeys.command.quickSwitcher', defaults: ['Ctrl+KeyO'] },
  { id: 'note.new', scope: 'window', label: 'hotkeys.command.newNote', defaults: ['Ctrl+KeyN'] },
  {
    id: 'editor.toggle-mode',
    scope: 'window',
    label: 'hotkeys.command.toggleMode',
    hint: 'hotkeys.command.toggleMode.hint',
    defaults: ['Ctrl+KeyE'],
  },
  { id: 'history.back', scope: 'window', label: 'hotkeys.command.back', defaults: ['Alt+ArrowLeft'] },
  { id: 'history.forward', scope: 'window', label: 'hotkeys.command.forward', defaults: ['Alt+ArrowRight'] },
  { id: 'tree.rename', scope: 'tree', label: 'hotkeys.command.rename', defaults: ['F2'] },
  { id: 'tree.delete', scope: 'tree', label: 'hotkeys.command.delete', defaults: ['Delete'] },
];

/** Системные сочетания: показываются с замком, не переназначаются. */
export const SYSTEM_ROWS: readonly { label: MessageKey; chords: readonly string[] }[] = [
  { label: 'hotkeys.system.undo', chords: ['Ctrl+KeyZ', 'Ctrl+KeyY'] },
  { label: 'hotkeys.system.clipboard', chords: ['Ctrl+KeyC', 'Ctrl+KeyX', 'Ctrl+KeyV'] },
  { label: 'hotkeys.system.closeWindow', chords: ['Alt+F4'] },
];

/** Клавиша события, важная для сочетаний. */
export interface KeyLike {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
  isComposing?: boolean;
}

const KEYS: ReadonlySet<string> = new Set(LIMITS.hotkey_keys);
const BARE_KEYS: ReadonlySet<string> = new Set(LIMITS.hotkey_bare_keys);
const RESERVED: ReadonlySet<string> = new Set(LIMITS.hotkey_reserved);

/**
 * Сочетание нажатия; `null` — нажат только модификатор, клавиша не
 * назначается (Esc, Tab) или идёт набор через IME. Cmd (macOS) — как Ctrl.
 */
export function chordFromEvent(event: KeyLike): string | null {
  if (event.isComposing) return null;
  let code = event.code;
  // Ввод через службу текста Windows может прислать пустой code: тогда — по
  // key: имя клавиши (F2, Delete, ArrowLeft — то же, что code) или латинская
  // буква и цифра.
  if (!KEYS.has(code)) {
    const letter = event.key.length === 1 ? event.key.toUpperCase() : '';
    if (KEYS.has(event.key)) code = event.key;
    else if (/^[A-Z]$/.test(letter)) code = `Key${letter}`;
    else if (/^[0-9]$/.test(letter)) code = `Digit${letter}`;
    else return null;
  }
  const parts: string[] = [];
  if (event.ctrlKey || event.metaKey) parts.push('Ctrl');
  if (event.altKey) parts.push('Alt');
  if (event.shiftKey) parts.push('Shift');
  parts.push(code);
  return parts.join('+');
}

export type ChordProblem = 'reserved' | 'needs_modifier';

/** Можно ли назначить сочетание: те же правила, что в ядре. */
export function checkChord(chord: string): ChordProblem | null {
  if (RESERVED.has(chord)) return 'reserved';
  const parts = chord.split('+');
  const key = parts.at(-1) ?? '';
  const commandModifier = parts.includes('Ctrl') || parts.includes('Alt');
  if (!commandModifier && !BARE_KEYS.has(key)) return 'needs_modifier';
  return null;
}

function command(id: CommandId): HotkeyCommand {
  const found = COMMANDS.find((c) => c.id === id);
  if (!found) throw new Error(`нет команды ${id}`);
  return found;
}

/** Сочетания команды: назначенные пользователем или по умолчанию. */
export function bindingsOf(overrides: HotkeyOverrides, id: CommandId): readonly string[] {
  return Object.hasOwn(overrides, id) ? (overrides[id] ?? []) : command(id).defaults;
}

/** Сочетания команды отличаются от стандартных. */
export function isChanged(overrides: HotkeyOverrides, id: CommandId): boolean {
  const current = bindingsOf(overrides, id);
  const defaults = command(id).defaults;
  return current.length !== defaults.length || current.some((chord, i) => chord !== defaults[i]);
}

/** Команда, у которой есть это сочетание (кроме `except`). */
export function ownerOf(overrides: HotkeyOverrides, chord: string, except?: CommandId): HotkeyCommand | null {
  return COMMANDS.find((c) => c.id !== except && bindingsOf(overrides, c.id).includes(chord)) ?? null;
}

/** Ставит сочетания команды; совпадающие со стандартными не хранятся. */
function withBindings(overrides: HotkeyOverrides, id: CommandId, chords: readonly string[]): HotkeyOverrides {
  const next = { ...overrides, [id]: [...chords] };
  return isChanged(next, id) ? next : resetCommand(next, id);
}

/**
 * Назначает `chord` команде: вместо `replacing` или ещё одним (`null`).
 * Если сочетание было у другой команды, у неё оно снимается («Заменить»).
 */
export function assignChord(
  overrides: HotkeyOverrides,
  id: CommandId,
  chord: string,
  replacing: string | null,
): HotkeyOverrides {
  let next = overrides;
  const owner = ownerOf(next, chord, id);
  if (owner) next = withBindings(next, owner.id, bindingsOf(next, owner.id).filter((c) => c !== chord));
  const current = bindingsOf(next, id).filter((c) => c !== chord);
  const index = replacing === null ? -1 : current.indexOf(replacing);
  const chords = index >= 0 ? current.map((c, i) => (i === index ? chord : c)) : [...current, chord];
  return withBindings(next, id, chords.slice(0, LIMITS.hotkey_max_chords));
}

/** Убирает сочетание у команды. */
export function removeChord(overrides: HotkeyOverrides, id: CommandId, chord: string): HotkeyOverrides {
  return withBindings(
    overrides,
    id,
    bindingsOf(overrides, id).filter((c) => c !== chord),
  );
}

/** Сочетания команды — стандартные. */
export function resetCommand(overrides: HotkeyOverrides, id: CommandId): HotkeyOverrides {
  return Object.fromEntries(Object.entries(overrides).filter(([command]) => command !== id));
}

/** Команда области `scope` для нажатия; `null` — нажатие не команда. */
export function matchCommand(overrides: HotkeyOverrides, event: KeyLike, scope: HotkeyScope): CommandId | null {
  const chord = chordFromEvent(event);
  if (!chord) return null;
  const found = COMMANDS.find((c) => c.scope === scope && bindingsOf(overrides, c.id).includes(chord));
  return found?.id ?? null;
}

const KEY_LABELS: Readonly<Record<string, string>> = {
  ArrowLeft: '←',
  ArrowRight: '→',
  ArrowUp: '↑',
  ArrowDown: '↓',
  PageUp: 'PgUp',
  PageDown: 'PgDn',
  Minus: '-',
  Equal: '=',
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
  Backquote: '`',
};

function keyLabel(code: string): string {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  return KEY_LABELS[code] ?? code;
}

/** Клавиши сочетания для показа: `['Ctrl', 'Shift', 'E']`. */
export function chordParts(chord: string): string[] {
  const parts = chord.split('+');
  const key = parts.pop() ?? '';
  return [...parts, keyLabel(key)];
}

/** Сочетание строкой: `Ctrl+O`. */
export function chordText(chord: string): string {
  return chordParts(chord).join('+');
}
