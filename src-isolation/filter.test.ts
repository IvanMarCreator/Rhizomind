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

const message = (cmd: string): IpcMessage => ({ cmd, callback: 1, error: 2, payload: { path: 'x.md' } });

describe('фильтр изолированного iframe', () => {
  it('пропускает служебные команды событий без изменений', () => {
    const hook = loadHook();
    for (const cmd of ['plugin:event|listen', 'plugin:event|unlisten']) {
      expect(hook(message(cmd))).toEqual(message(cmd));
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
