// Хранилище для замера производительности (задача 3.2, С9.5): 10 000 заметок.
//
// 9 999 заметок создаёт tools/gen-vault (зерно 1 — каждый прогон одно и то же
// хранилище), десятитысячная — «Длинная заметка 5000 строк.md» в корне: для
// критерия «набор текста в заметке на 5 000 строк». Её текст собран из целых
// заметок того же хранилища (заголовки, списки, ссылки, код), чтобы живой
// предпросмотр размечал обычный Markdown, а не одну повторённую строку.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const ROOT = path.join(import.meta.dirname, '..', '..');
export const NOTES = 10_000;
export const LONG_LINES = 5_000;
export const SEED = 1;
export const VAULT_NAME = 'Нагрузка 10000';
export const LONG_TITLE = 'Длинная заметка 5000 строк';
/** Первая строка длинной заметки: по ней видно, что заметка открыта. */
export const LONG_MARKER = 'Длинная заметка для замера ввода';

export interface PerfVault {
  root: string;
  /** Записей в корне без скрытых: столько строк у дерева сразу после открытия. */
  rootEntries: number;
  /** Запросы быстрого перехода: имена настоящих заметок без пробела. */
  queries: string[];
  /** Сколько заняла генерация, мс. */
  generateMs: number;
}

/** gen-vault: RHIZOMIND_PERF_GEN_VAULT или выпускная сборка в папке Cargo. */
function genVaultPath(): string {
  if (process.env.RHIZOMIND_PERF_GEN_VAULT) return process.env.RHIZOMIND_PERF_GEN_VAULT;
  const target = process.env.CARGO_TARGET_DIR
    ? path.resolve(ROOT, process.env.CARGO_TARGET_DIR)
    : path.join(ROOT, 'src-tauri', 'target');
  return path.join(target, 'release', process.platform === 'win32' ? 'gen-vault.exe' : 'gen-vault');
}

/** Все заметки (.md) хранилища, пути от корня через `/`, по порядку; без скрытых. */
function notesOf(root: string): string[] {
  const out: string[] = [];
  const walk = (rel: string) => {
    for (const entry of readdirSync(path.join(root, rel), { withFileTypes: true })) {
      if (entry.name.startsWith('.')) continue;
      const child = rel ? `${rel}/${entry.name}` : entry.name;
      if (entry.isDirectory()) walk(child);
      else if (entry.name.endsWith('.md')) out.push(child);
    }
  };
  walk('');
  return out.sort();
}

/** Текст длинной заметки: ровно `LONG_LINES` строк с LF. */
export function longNoteText(sources: readonly string[]): string {
  const lines = [`# ${LONG_MARKER}`, ''];
  for (const source of sources) {
    const body = source.replace(/^\uFEFF/, '').replace(/\r\n?/g, '\n').replace(/\n+$/, '').split('\n');
    // Только заметки с закрытыми блоками кода: иначе остаток текста стал бы кодом.
    if (body.filter((line) => /^\s*(```|~~~)/.test(line)).length % 2 !== 0) continue;
    if (lines.length + body.length + 1 > LONG_LINES) break;
    lines.push(...body, '');
  }
  for (let i = 1; lines.length < LONG_LINES; i++) lines.push(`Строка дополнения ${i}: обычный абзац без разметки.`);
  return `${lines.join('\n')}\n`;
}

export function prepareVault(): PerfVault {
  const parent = mkdtempSync(path.join(os.tmpdir(), 'rhizomind-perf-'));
  const root = path.join(parent, VAULT_NAME);
  const started = Date.now();
  execFileSync(
    genVaultPath(),
    ['--notes', String(NOTES - 1), '--seed', String(SEED), '--out', root, '--manifest', path.join(parent, 'manifest.json')],
    { stdio: 'inherit' },
  );
  const generateMs = Date.now() - started;
  const notes = notesOf(root);
  writeFileSync(
    path.join(root, `${LONG_TITLE}.md`),
    longNoteText(notes.slice(0, 2_000).map((note) => readFileSync(path.join(root, note), 'utf8'))),
  );
  const count = notesOf(root).length;
  if (count !== NOTES) throw new Error(`в хранилище ${count} заметок, а нужно ${NOTES}`);

  // Пять имён вида «Слово 123» через равные промежутки списка; запрос — имя
  // без пробела строчными («протокол422»): пробел в конце запроса не меняет
  // список (ядро делит запрос по пробелам), а замер ждёт изменения списка.
  const names = notes
    .map((note) => path.posix.basename(note, '.md'))
    .filter((name) => name === name.normalize('NFC') && /^\p{L}+ \d+$/u.test(name));
  const queries = [0, 1, 2, 3, 4].map((i) => {
    const name = names[Math.floor(((i + 0.5) * names.length) / 5)];
    if (name === undefined) throw new Error('мало имён для запросов быстрого перехода');
    return name.replace(' ', '').toLowerCase();
  });

  const rootEntries = readdirSync(root).filter((name) => !name.startsWith('.')).length;
  return { root, rootEntries, queries, generateMs };
}

export function removeVault(vault: PerfVault): void {
  rmSync(path.dirname(vault.root), { recursive: true, force: true });
}
