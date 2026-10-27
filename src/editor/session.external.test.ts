// Внешнее изменение открытой заметки (С3.6, задача 2.9, инвариант 4):
// без несохранённого ввода текст обновляется с диска; с вводом — ввод
// записывается сразу, и ядро сохраняет обе версии (версия с диска — копией).
// Подменный «диск» ведёт версии так же, как ядро: запись со старой
// ожидаемой версией — `conflict_saved`, версия с диска уходит в копию.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FileVersion, NoteReadResponse, NoteWriteRequest, NoteWriteResponse, VaultPath } from '../api/index.ts';
import { AUTOSAVE_DELAY_MS, NoteSession, type NoteStore } from './session.ts';

const PATH: VaultPath = ['Встреча.md'];
const COPY: VaultPath = ['Встреча (внешняя версия 2026-10-27 10-00).md'];

function version(generation: number): FileVersion {
  return { hash: String(generation).padStart(64, '0'), size: generation, modified_ms: generation };
}

class Disk implements NoteStore {
  text = 'с диска\n';
  generation = 1;
  read_only: NoteReadResponse['read_only'] = null;
  copies: string[] = [];
  reads = 0;
  writes: NoteWriteRequest[] = [];
  holdReads = false;
  holdWrites = false;
  readonly #heldReads: (() => void)[] = [];
  readonly #heldWrites: (() => void)[] = [];

  /** Правка «снаружи» (другой редактор): новое содержимое и новая версия. */
  external(text: string): FileVersion {
    this.text = text;
    this.generation += 1;
    return version(this.generation);
  }

  read(): Promise<NoteReadResponse> {
    this.reads += 1;
    const answer = (): NoteReadResponse => ({ text: this.text, version: version(this.generation), read_only: this.read_only });
    if (!this.holdReads) return Promise.resolve().then(answer);
    return new Promise((resolve) => this.#heldReads.push(() => resolve(answer())));
  }

  write(request: NoteWriteRequest): Promise<NoteWriteResponse> {
    this.writes.push(request);
    const answer = (): NoteWriteResponse => {
      const conflict = request.expected_version.hash !== version(this.generation).hash;
      if (conflict) this.copies.push(this.text);
      this.text = request.text;
      this.generation += 1;
      const next = version(this.generation);
      return conflict ? { outcome: 'conflict_saved', version: next, copy_path: COPY } : { outcome: 'saved', version: next };
    };
    if (!this.holdWrites) return Promise.resolve().then(answer);
    return new Promise((resolve) => this.#heldWrites.push(() => resolve(answer())));
  }

  releaseReads(): void {
    for (const go of this.#heldReads.splice(0)) go();
  }

  releaseWrites(): void {
    for (const go of this.#heldWrites.splice(0)) go();
  }
}

function open(disk: Disk) {
  const shown: string[] = [];
  const conflicts: VaultPath[] = [];
  const s = new NoteSession(PATH, { text: disk.text, version: version(disk.generation), read_only: disk.read_only }, disk, {
    onConflictSaved: (copy) => conflicts.push(copy),
  });
  s.onExternalText((text) => shown.push(text));
  return { s, shown, conflicts };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('внешняя правка без несохранённого ввода', () => {
  it('текст и версия — с диска; редактору уходит новый текст; на диск ничего не пишется', async () => {
    const disk = new Disk();
    const { s, shown } = open(disk);

    const changed = disk.external('снаружи\n');
    expect(await s.external(changed)).toEqual({ kind: 'reloaded' });

    expect(s.text).toBe('снаружи\n');
    expect(shown).toEqual(['снаружи\n']);
    expect(s.status.kind).toBe('saved');
    expect(s.hasUnsavedChanges).toBe(false);
    await vi.advanceTimersByTimeAsync(5 * AUTOSAVE_DELAY_MS);
    expect(disk.writes).toEqual([]);

    // Следующая правка пишется с новой версией — без ложного конфликта.
    s.edit('снаружи\nи моё\n');
    await s.flush();
    expect(disk.writes[0]?.expected_version).toEqual(changed);
    expect(disk.copies).toEqual([]);
  });

  it('версия события совпала с известной (своя запись) — не перечитывает', async () => {
    const disk = new Disk();
    const { s, shown } = open(disk);
    s.edit('моё\n');
    await s.flush();

    expect(await s.external(version(disk.generation))).toEqual({ kind: 'none' });
    expect(disk.reads).toBe(0);
    expect(shown).toEqual([]);
  });

  it('полная сверка (версии нет): перечитывает и обновляет, только если файл другой', async () => {
    const disk = new Disk();
    const { s, shown } = open(disk);

    expect(await s.external()).toEqual({ kind: 'none' });
    expect(shown).toEqual([]);

    disk.external('после сверки\n');
    expect(await s.external()).toEqual({ kind: 'reloaded' });
    expect(shown).toEqual(['после сверки\n']);
  });

  it('стала только для чтения (или наоборот) — сессию надо открыть заново', async () => {
    const disk = new Disk();
    const { s, shown } = open(disk);

    disk.read_only = { reason: 'invalid_utf8' };
    const changed = disk.external('�\n');
    const outcome = await s.external(changed);

    expect(outcome.kind).toBe('reopen');
    expect(shown).toEqual([]);
  });
});

describe('внешняя правка при несохранённом вводе: обе версии (С3.6)', () => {
  it('ввод записывается сразу со старой версией — ядро сохраняет обе версии', async () => {
    const disk = new Disk();
    const { s, shown, conflicts } = open(disk);
    s.edit('мой ввод\n');

    const changed = disk.external('снаружи\n');
    expect(await s.external(changed)).toEqual({ kind: 'flushed' });

    expect(disk.text).toBe('мой ввод\n');
    expect(disk.copies).toEqual(['снаружи\n']);
    expect(conflicts).toEqual([COPY]);
    expect(s.text).toBe('мой ввод\n');
    expect(shown).toEqual([]);
    expect(s.hasUnsavedChanges).toBe(false);
  });

  it('ввод во время чтения с диска: прочитанное не показывается, ввод не теряется — обе версии', async () => {
    const disk = new Disk();
    const { s, shown, conflicts } = open(disk);
    disk.holdReads = true;

    const changed = disk.external('снаружи\n');
    const pending = s.external(changed);
    await vi.advanceTimersByTimeAsync(0);
    s.edit('с диска\nввод во время чтения\n');
    disk.releaseReads();
    expect(await pending).toEqual({ kind: 'flushed' });

    expect(shown).toEqual([]);
    expect(s.text).toBe('с диска\nввод во время чтения\n');
    await s.flush();
    expect(disk.text).toBe('с диска\nввод во время чтения\n');
    expect(disk.copies).toEqual(['снаружи\n']);
    expect(conflicts).toEqual([COPY]);
  });

  it('правка снаружи во время автосохранения: запись в полёте, событие ждёт её ответа', async () => {
    const disk = new Disk();
    const { s, shown, conflicts } = open(disk);
    s.edit('мой ввод\n');
    disk.holdWrites = true;
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS);
    expect(s.writing).toBe(true);

    // Снаружи записали раньше, чем запрос дошёл до диска.
    const changed = disk.external('снаружи\n');
    const pending = s.external(changed);
    disk.releaseWrites();
    await pending;

    expect(disk.text).toBe('мой ввод\n');
    expect(disk.copies).toEqual(['снаружи\n']);
    expect(conflicts).toEqual([COPY]);
    expect(shown).toEqual([]);
    expect(s.text).toBe('мой ввод\n');
  });

  it('несколько событий подряд не перечитывают и не пишут наперегонки', async () => {
    const disk = new Disk();
    const { s, shown } = open(disk);

    const first = disk.external('раз\n');
    const second = disk.external('два\n');
    await Promise.all([s.external(first), s.external(second)]);

    expect(s.text).toBe('два\n');
    expect(shown.at(-1)).toBe('два\n');
    expect(disk.writes).toEqual([]);
  });
});

describe('удалённая или закрытая сессия', () => {
  it('после discard (удаление) внешняя правка ничего не делает', async () => {
    const disk = new Disk();
    const { s, shown } = open(disk);
    s.discard();

    expect(await s.external(disk.external('снаружи\n'))).toEqual({ kind: 'none' });
    expect(disk.reads).toBe(0);
    expect(shown).toEqual([]);
  });

  it('отписка от нового текста', async () => {
    const disk = new Disk();
    const s = new NoteSession(PATH, { text: disk.text, version: version(1), read_only: null }, disk);
    const shown: string[] = [];
    const stop = s.onExternalText((text) => shown.push(text));
    stop();

    await s.external(disk.external('снаружи\n'));
    expect(shown).toEqual([]);
    expect(s.text).toBe('снаружи\n');
  });
});
