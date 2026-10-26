// Фильтр вызовов ядра в изолированном iframe (isolation pattern, ADR 0009).
// Код iframe — обычный скрипт без модулей (на Windows ES-модули внутри
// isolation не работают), поэтому тест выполняет его в отдельном контексте.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import { describe, expect, it } from 'vitest';

interface IpcMessage {
  cmd: string;
  callback: number;
  error: number;
  payload: unknown;
  options?: unknown;
}
type Hook = (message: IpcMessage) => IpcMessage;

function loadHook(): Hook {
  const code = readFileSync(path.join(import.meta.dirname, 'index.js'), 'utf8');
  const window: { __TAURI_ISOLATION_HOOK__?: Hook } = {};
  vm.runInNewContext(code, { window });
  if (typeof window.__TAURI_ISOLATION_HOOK__ !== 'function') throw new Error('хук не задан');
  return window.__TAURI_ISOLATION_HOOK__;
}

/** Команды ядра API v1 из единственного списка `src-tauri/src/commands/list.rs`. */
function coreCommands(): string[] {
  const list = readFileSync(path.join(import.meta.dirname, '..', 'src-tauri', 'src', 'commands', 'list.rs'), 'utf8');
  const body = /\$callback!\(([\s\S]*?)\)/.exec(list)?.[1];
  if (body === undefined) throw new Error('в list.rs не найден список команд');
  return body
    .split('\n')
    .map((line) => line.replace(/\/\/.*$/, '').trim())
    .join(' ')
    .split(',')
    .map((name) => name.trim())
    .filter((name) => name !== '');
}

/** Литерал OWN_COMMANDS из кода фильтра — чтобы сверить список целиком. */
function ownCommandsInFilter(): string[] {
  const code = readFileSync(path.join(import.meta.dirname, 'index.js'), 'utf8');
  const literal = /var OWN_COMMANDS = Object\.freeze\(\[([\s\S]*?)\]\)/.exec(code)?.[1];
  if (literal === undefined) throw new Error('в index.js не найден OWN_COMMANDS');
  return [...literal.matchAll(/'([^']*)'/g)].map((m) => m[1] as string);
}

const message = (cmd: string): IpcMessage => ({ cmd, callback: 1, error: 2, payload: { path: 'x.md' } });

describe('фильтр изолированного iframe', () => {
  it('пропускает служебные команды событий без изменений', () => {
    const hook = loadHook();
    const listen = {
      cmd: 'plugin:event|listen',
      callback: 1,
      error: 2,
      payload: { event: 'vault:changed', target: { kind: 'WebviewWindow', label: 'vault-0123456789abcdef' }, handler: 3 },
    };
    expect(hook(listen)).toEqual(listen);
    const unlisten = { cmd: 'plugin:event|unlisten', callback: 1, error: 2, payload: { event: 'vault:changed', eventId: 3 } };
    expect(hook(unlisten)).toEqual(unlisten);
  });

  // Задача 1.3: слушатель с целью Any получал бы события чужих окон (Tauri
  // отдаёт ему и события, отправленные другому окну через emit_to).
  it('не пропускает подписку на события без цели «окно с меткой»', () => {
    const hook = loadHook();
    for (const target of [
      undefined,
      null,
      { kind: 'Any' },
      { kind: 'AnyLabel', label: 'vault-0123456789abcdef' },
      { kind: 'App' },
      { kind: 'Window', label: 'vault-0123456789abcdef' },
      { kind: 'Webview', label: 'vault-0123456789abcdef' },
      { kind: 'WebviewWindow' },
      { kind: 'WebviewWindow', label: '' },
      { kind: 'WebviewWindow', label: 42 },
      'vault-0123456789abcdef',
    ]) {
      const out = hook({ cmd: 'plugin:event|listen', callback: 1, error: 2, payload: { event: 'vault:changed', target, handler: 3 } });
      expect(out.cmd, JSON.stringify(target)).toBe('__rhizomind_isolation_rejected__');
      expect(out.payload).toEqual({});
    }
    expect(hook({ cmd: 'plugin:event|listen', callback: 1, error: 2, payload: null }).cmd).toBe(
      '__rhizomind_isolation_rejected__',
    );
  });

  it('пропускает ровно команды ядра из src-tauri/src/commands/list.rs', () => {
    const commands = coreCommands();
    expect(commands.length).toBeGreaterThan(0);
    expect([...ownCommandsInFilter()].sort()).toEqual([...commands].sort());
    const hook = loadHook();
    for (const cmd of commands) {
      expect(hook(message(cmd)), cmd).toEqual(message(cmd));
    }
  });

  it('не пропускает команды ядра с изменённым написанием', () => {
    const hook = loadHook();
    for (const cmd of coreCommands().flatMap((c) => [c.toUpperCase(), ` ${c}`, `${c} `, `plugin:${c}`])) {
      expect(hook(message(cmd)).cmd, cmd).toBe('__rhizomind_isolation_rejected__');
    }
  });

  it('подменяет любую другую команду на заведомо несуществующую и стирает параметры', () => {
    const hook = loadHook();
    for (const cmd of [
      'read_note',
      'plugin:fs|read_file',
      'plugin:opener|open_url',
      'plugin:shell|execute',
      'plugin:app|version',
      'plugin:event|emit',
      'plugin:event|listen ',
      'PLUGIN:EVENT|LISTEN',
      '',
    ]) {
      const out = hook(message(cmd));
      expect(out.cmd, cmd).toBe('__rhizomind_isolation_rejected__');
      expect(out.payload, cmd).toEqual({});
      expect(out.callback).toBe(1);
      expect(out.error).toBe(2);
    }
  });

  it('отклоняет сообщения неожиданной формы', () => {
    const hook = loadHook();
    expect(hook({ cmd: 42 } as unknown as IpcMessage).cmd).toBe('__rhizomind_isolation_rejected__');
    expect(hook(null as unknown as IpcMessage).cmd).toBe('__rhizomind_isolation_rejected__');
  });

  it('список разрешённых нельзя изменить извне', () => {
    const code = readFileSync(path.join(import.meta.dirname, 'index.js'), 'utf8');
    expect(code).toContain('Object.freeze');
    expect(code).not.toMatch(/\bimport\b|\bexport\b/);
  });
});
