// Хранилище для замера производительности (задача 3.2, С9.5): 10 000 заметок.
//
// 9 999 заметок создаёт tools/gen-vault (зерно 1 — каждый прогон одно и то же
// хранилище), десятитысячная — «Длинная заметка 5000 строк.md» в корне: для
// критерия «набор текста в заметке на 5 000 строк». Её текст собран из целых
// заметок того же хранилища (заголовки, списки, ссылки, код), чтобы живой
// предпросмотр размечал обычный Markdown, а не одну повторённую строку.
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { findTags, tagKey } from '../../src/core/tags/index.ts';

const ROOT = path.join(import.meta.dirname, '..', '..');
export const NOTES = 10_000;
export const LONG_LINES = 5_000;
export const SEED = 1;
export const LONG_TITLE = 'Длинная заметка 5000 строк';
/** Первая строка длинной заметки: по ней видно, что заметка открыта. */
export const LONG_MARKER = 'Длинная заметка для замера ввода';

export interface PerfVault {
  root: string;
  fixture: ReturnType<typeof releaseFixture>;
  tagKeys: string[];
  movedLinks: { source: string; oldLink: string; newLink: string; expectedCount: number; target: string }[];
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

interface ReleaseManifest {
  format_version: number;
  notes: number;
  tags: { key: string }[];
  hub: { path: string; backlinks: { source: string }[] } | null;
  folder_1000: { path: string; notes: number; incoming_links: number } | null;
}

/** Проверить, что бинарник генератора действительно подготовил нагрузку С2-8.3. */
export function releaseFixture(manifest: ReleaseManifest, count: number) {
  const folder = manifest.folder_1000;
  const hub = manifest.hub;
  const hubSources = new Set(hub?.backlinks.map((link) => link.source));
  if (manifest.format_version !== 2 || manifest.notes !== count - 1 || !manifest.tags?.length ||
      !hub || hubSources.size !== 100 || !folder || folder.notes !== 1_000 || folder.incoming_links !== 5_000) {
    throw new Error('Манифест не соответствует нагрузке С2-8.3');
  }
  return { hub: path.posix.basename(hub.path, '.md'), hubSources, folder: folder.path };
}

export const MOVE_TARGET = 'Приёмник перемещения';

/** Манифест описывает сгенерированные файлы, а длинную заметку добавляет
 * сам оконный тест. Бывший блок свойств посреди склеенного текста — уже абзац. */
export function expectedTagKeys(manifestKeys: string[], longText: string): string[] {
  return [...new Set([...manifestKeys, ...findTags(longText).map((tag) => tagKey(tag.name))])].sort();
}

/** Независимые свидетели переноса: абсолютный путь от корня перестаёт работать,
 * уникальная заметка генератора получает кратчайшую ссылку по имени. */
function moveExamples(root: string, notes: string[], folder: string): PerfVault['movedLinks'] {
  const samples: PerfVault['movedLinks'] = [];
  for (const source of notes) {
    if (source.startsWith(`${folder}/`)) continue;
    const text = readFileSync(path.join(root, source), 'utf8');
    for (const [oldLink, target] of text.matchAll(/\[\[\/([^\]|#]+)\]\]/gu)) {
      if (!target?.startsWith(`${folder}/`)) continue;
      const newLink = `[[${path.posix.basename(target)}]]`;
      samples.push({ source, oldLink, newLink, target: `${target}.md`,
        expectedCount: text.split(newLink).length + text.split(oldLink).length - 2 });
      break;
    }
    if (samples.length === 3) return samples;
  }
  throw new Error('В нагрузке нет трёх независимых исходных заметок с абсолютной ссылкой в папку');
}

export function prepareVault(count = NOTES): PerfVault {
  const parent = mkdtempSync(path.join(os.tmpdir(), 'rhizomind-perf-'));
  try {
    return generateIn(parent, count);
  } catch (error) {
    rmSync(parent, { recursive: true, force: true });
    throw error;
  }
}

function generateIn(parent: string, count: number): PerfVault {
  const root = path.join(parent, `Нагрузка ${count}`);
  const started = Date.now();
  execFileSync(
    genVaultPath(),
    ['--notes', String(count - 1), '--seed', String(SEED), '--out', root, '--manifest', path.join(parent, 'manifest.json')],
    { stdio: 'inherit' },
  );
  const generateMs = Date.now() - started;
  const notes = notesOf(root);
  const manifest = JSON.parse(readFileSync(path.join(parent, 'manifest.json'), 'utf8')) as ReleaseManifest;
  const fixture = releaseFixture(manifest, count);
  mkdirSync(path.join(root, MOVE_TARGET));
  // При копировании текста в длинную заметку не добавлять 101-й источник
  // узла и дополнительные входящие ссылки в папку с нагрузкой 5 000.
  const folderNames = new Set(notes.filter((note) => note.startsWith(`${fixture.folder}/`))
    .map((note) => path.posix.basename(note, '.md').normalize('NFC').toLowerCase()));
  const sources = notes.filter((note) => !fixture.hubSources.has(note)).slice(0, 2_000)
    .map((note) => readFileSync(path.join(root, note), 'utf8'))
    .filter((text) => ![...text.matchAll(/\[\[([^|#\]]+)/g)].some((match) =>
      folderNames.has(path.posix.basename(match[1] ?? '', '.md').normalize('NFC').toLowerCase())));
  const longText = longNoteText(sources);
  writeFileSync(path.join(root, `${LONG_TITLE}.md`), longText);
  const tagKeys = expectedTagKeys(manifest.tags.map((tag) => tag.key), longText);
  if (notes.length + 1 !== count) throw new Error(`в хранилище ${notes.length + 1} заметок, а нужно ${count}`);

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
  return { root, fixture, tagKeys, movedLinks: moveExamples(root, notes, fixture.folder), rootEntries, queries, generateMs };
}

export function removeVault(vault: PerfVault): void {
  rmSync(path.dirname(vault.root), { recursive: true, force: true });
}
