// Проверка формата общих образцов разбора tests/fixtures/ (задача 0.2-0.3) по
// описанию из tests/fixtures/README.md: форма каждого файла, согласованность
// смещений UTF-16 и байтовых, таблица покрытия С2-3.3, байты файлов заметок
// relink/notes/ (git не должен был поменять CRLF, BOM и концевые пробелы).
// Сам разбор здесь не проверяется — это делают тесты ядра и интерфейса.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

const ROOT = join(import.meta.dirname, '..', '..', 'tests', 'fixtures');

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
type Obj = Record<string, Json>;

function isObj(value: Json | undefined): value is Obj {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function obj(value: Json | undefined, where: string): Obj {
  if (!isObj(value)) throw new Error(`${where}: ожидался объект`);
  return value;
}

function str(value: Json | undefined, where: string): string {
  if (typeof value !== 'string') throw new Error(`${where}: ожидалась строка`);
  return value;
}

function int(value: Json | undefined, where: string): number {
  if (typeof value !== 'number' || !Number.isInteger(value) || value < 0) throw new Error(`${where}: ожидалось целое ≥ 0`);
  return value;
}

function arr(value: Json | undefined, where: string): Json[] {
  if (!Array.isArray(value)) throw new Error(`${where}: ожидался массив`);
  return value;
}

function keys(o: Obj, expected: string[], where: string): void {
  expect(Object.keys(o).sort(), `${where}: набор полей`).toEqual([...expected].sort());
}

function listJson(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return listJson(path);
    return name.endsWith('.json') ? [path] : [];
  });
}

interface Fixture {
  cases: Obj[];
}

function load(file: string): Fixture {
  const data = obj(JSON.parse(readFileSync(join(ROOT, file), 'utf8')) as Json, file);
  return { cases: arr(data.cases, `${file}: cases`).map((c, i) => obj(c, `${file}: cases[${i}]`)) };
}

/** Участок текста: смещения UTF-16 и байтовые указывают на одну и ту же подстроку. */
function range(text: string, value: Json | undefined, where: string): { from: number; to: number; text: string } {
  const r = obj(value, where);
  keys(r, ['from', 'to', 'byte_from', 'byte_to'], where);
  const from = int(r.from, `${where}.from`);
  const to = int(r.to, `${where}.to`);
  const byteFrom = int(r.byte_from, `${where}.byte_from`);
  const byteTo = int(r.byte_to, `${where}.byte_to`);
  expect(from <= to && to <= text.length, `${where}: from ≤ to ≤ длины текста`).toBe(true);
  const sub = text.slice(from, to);
  const bytes = Buffer.from(text, 'utf8');
  expect(byteTo <= bytes.length, `${where}: byte_to в пределах текста`).toBe(true);
  expect(bytes.subarray(byteFrom, byteTo).toString('utf8'), `${where}: байтовые смещения`).toBe(sub);
  expect(Buffer.byteLength(text.slice(0, from), 'utf8'), `${where}: byte_from = from`).toBe(byteFrom);
  return { from, to, text: sub };
}

function ascending(ranges: { from: number; to: number }[], where: string): void {
  for (let i = 1; i < ranges.length; i++) {
    const prev = ranges[i - 1];
    const cur = ranges[i];
    if (prev && cur) expect(prev.to <= cur.from, `${where}: по порядку, без пересечений`).toBe(true);
  }
}

/** Идентификаторы правил из первого столбца таблиц README (`| Т1 |`, `| Ф1 |`). */
function readmeRules(prefix: string): string[] {
  const readme = readFileSync(join(ROOT, 'README.md'), 'utf8');
  const re = new RegExp(`^\\| (${prefix}\\d+) \\|`, 'gmu');
  return [...readme.matchAll(re)].map((m) => m[1] ?? '');
}

describe('образцы: общий вид файлов', () => {
  const files = listJson(ROOT);

  it('нашлись все файлы образцов', () => {
    const names = files.map((f) => relative(ROOT, f).replaceAll('\\', '/')).sort();
    expect(names).toEqual(
      expect.arrayContaining([
        'frontmatter/tags.json',
        'headings/text.json',
        'relink/cases.json',
        'relink/links.json',
        'tags/key.json',
        'tags/text.json',
        'wikilinks/parse.json',
        'wikilinks/text.json',
      ]),
    );
  });

  it.each(files.map((f) => [relative(ROOT, f).replaceAll('\\', '/'), f]))('%s: format_version 1, description, cases; UTF-8 без BOM, LF', (_name, path) => {
    const bytes = readFileSync(path);
    expect(bytes[0], 'без BOM').not.toBe(0xef);
    expect(bytes.includes(0x0d), 'без CR: JSON хранится с LF').toBe(false);
    const data = obj(JSON.parse(bytes.toString('utf8')) as Json, path);
    expect(data.format_version).toBe(1);
    expect(str(data.description, 'description').length).toBeGreaterThan(0);
    expect(arr(data.cases, 'cases').length).toBeGreaterThan(0);
  });
});

describe('tags/text.json', () => {
  const { cases } = load('tags/text.json');
  const table = readmeRules('Т');

  it('не меньше 60 образцов', () => expect(cases.length).toBeGreaterThanOrEqual(60));

  it.each(cases.map((c, i) => [i, str(c.note, 'note'), c]))('#%i %s', (i, _note, c) => {
    const where = `tags/text.json #${i}`;
    keys(c, ['rule', 'kind', 'note', 'text', 'tags'], where);
    const rule = str(c.rule, `${where}.rule`);
    expect(rule === 'Т0' || table.includes(rule), `${where}: правило ${rule} есть в таблице README`).toBe(true);
    const kind = str(c.kind, `${where}.kind`);
    expect(['tag', 'not_tag']).toContain(kind);
    const text = str(c.text, `${where}.text`);
    expect(text.startsWith('﻿'), `${where}: текст без BOM`).toBe(false);
    const tags = arr(c.tags, `${where}.tags`).map((t, k) => {
      const tag = obj(t, `${where}.tags[${k}]`);
      keys(tag, ['raw', 'name', 'from', 'to', 'byte_from', 'byte_to'], `${where}.tags[${k}]`);
      const raw = str(tag.raw, 'raw');
      const name = str(tag.name, 'name');
      const r = range(text, { from: tag.from ?? null, to: tag.to ?? null, byte_from: tag.byte_from ?? null, byte_to: tag.byte_to ?? null }, `${where}.tags[${k}]`);
      expect(r.text, 'raw — подстрока по смещениям').toBe(raw);
      expect(raw, 'raw = # + name').toBe(`#${name}`);
      // Здравый смысл ожидаемого результата по С2-3.3 (ошибки в самих образцах).
      expect(name, 'тело тега по С2-3.3').toMatch(/^[\p{L}\p{N}\p{M}_\-/]+$/u);
      expect(name, 'хотя бы одна не-цифра').toMatch(/[^0-9]/u);
      expect(name.startsWith('/') || name.endsWith('/') || name.includes('//'), '/ не в начале, не в конце, не дважды').toBe(false);
      expect([...name].length, 'длина до 200').toBeLessThanOrEqual(200);
      return r;
    });
    ascending(tags, where);
    if (kind === 'tag') expect(tags.length, `${where}: образец «тег» содержит тег`).toBeGreaterThan(0);
  });

  it('таблица покрытия: каждое правило README — хотя бы один «тег» и один «не тег»', () => {
    expect(table.length).toBeGreaterThan(0);
    const missing = table.flatMap((rule) =>
      (['tag', 'not_tag'] as const).filter((kind) => !cases.some((c) => c.rule === rule && c.kind === kind)).map((kind) => `${rule}:${kind}`),
    );
    expect(missing).toEqual([]);
  });
});

describe('tags/key.json', () => {
  const { cases } = load('tags/key.json');
  it.each(cases.map((c, i) => [i, c]))('#%i', (i, c) => {
    const where = `tags/key.json #${i}`;
    keys(c, ['note', 'a', 'b', 'same'], where);
    str(c.a, `${where}.a`);
    str(c.b, `${where}.b`);
    expect(typeof c.same).toBe('boolean');
  });
});

describe('frontmatter/tags.json', () => {
  const { cases } = load('frontmatter/tags.json');
  const table = readmeRules('Ф');

  it('не меньше 15 образцов', () => expect(cases.length).toBeGreaterThanOrEqual(15));

  it.each(cases.map((c, i) => [i, str(c.note, 'note'), c]))('#%i %s', (i, _note, c) => {
    const where = `frontmatter/tags.json #${i}`;
    keys(c, ['rule', 'note', 'text', 'block', 'tags'], where);
    expect(table, `${where}: правило есть в таблице README`).toContain(str(c.rule, 'rule'));
    const text = str(c.text, `${where}.text`);
    expect(text.startsWith('﻿'), `${where}: текст без BOM`).toBe(false);
    if (c.block !== null) {
      const block = range(text, c.block, `${where}.block`);
      expect(block.from, 'блок — в самом начале').toBe(0);
      expect(block.text.startsWith('---'), 'блок начинается строкой ---').toBe(true);
      expect(block.text.endsWith('---'), 'блок кончается строкой --- без перевода строки').toBe(true);
    }
    const tags = arr(c.tags, `${where}.tags`).map((t) => str(t, 'tag'));
    for (const name of tags) expect(name).toMatch(/^[\p{L}\p{N}\p{M}_\-/]+$/u);
    if (c.block === null) expect(tags, `${where}: без блока тегов нет`).toEqual([]);
  });

  it('каждая строка таблицы «Блок свойств» покрыта образцом', () => {
    expect(table.filter((rule) => !cases.some((c) => c.rule === rule))).toEqual([]);
  });
});

describe('headings/text.json', () => {
  const { cases } = load('headings/text.json');
  it.each(cases.map((c, i) => [i, str(c.note, 'note'), c]))('#%i %s', (i, _note, c) => {
    const where = `headings/text.json #${i}`;
    keys(c, ['note', 'text', 'headings'], where);
    const text = str(c.text, `${where}.text`);
    const headings = arr(c.headings, `${where}.headings`).map((h, k) => {
      const w = `${where}.headings[${k}]`;
      const heading = obj(h, w);
      keys(heading, ['level', 'text', 'from', 'to', 'byte_from', 'byte_to'], w);
      const level = int(heading.level, `${w}.level`);
      expect(level >= 1 && level <= 6, `${w}: уровень 1–6`).toBe(true);
      const own = str(heading.text, `${w}.text`);
      const { level: _l, text: _t, ...pos } = heading;
      const r = range(text, pos, w);
      expect(r.text, `${w}: текст по смещениям`).toBe(own);
      expect(own, `${w}: без пробелов по краям и \\r`).toBe(own.trim());
      return r;
    });
    ascending(headings, where);
  });
});

describe('relink/links.json', () => {
  const { cases } = load('relink/links.json');
  it.each(cases.map((c, i) => [i, str(c.note, 'note'), c]))('#%i %s', (i, _note, c) => {
    const where = `relink/links.json #${i}`;
    keys(c, ['note', 'text', 'links'], where);
    const text = str(c.text, `${where}.text`);
    const links = arr(c.links, `${where}.links`).map((l, k) => {
      const w = `${where}.links[${k}]`;
      const link = obj(l, w);
      keys(link, ['raw', 'embed', 'from', 'to', 'byte_from', 'byte_to', 'target', 'heading', 'alias'], w);
      const { raw, embed, target, heading, alias, ...pos } = link;
      const r = range(text, pos, w);
      expect(r.text).toBe(str(raw, `${w}.raw`));
      expect(typeof embed).toBe('boolean');
      expect(r.text.startsWith(embed === true ? '![[' : '[['), `${w}: начало ссылки`).toBe(true);
      expect(r.text.endsWith(']]'), `${w}: конец ссылки`).toBe(true);
      const t = range(text, target, `${w}.target`);
      expect(t.from).toBe(r.from + (embed === true ? 3 : 2));
      const parts = [t];
      if (heading !== null) {
        const h = range(text, heading, `${w}.heading`);
        expect(text[h.from - 1], `${w}: заголовок после #`).toBe('#');
        parts.push(h);
      }
      if (alias !== null) {
        const a = range(text, alias, `${w}.alias`);
        expect(text[a.from - 1], `${w}: псевдоним после |`).toBe('|');
        expect(a.to).toBe(r.to - 2);
        parts.push(a);
      } else {
        expect((parts.at(-1) ?? t).to, `${w}: последняя часть до ]]`).toBe(r.to - 2);
      }
      ascending(parts, w);
      return r;
    });
    ascending(links, where);
  });
});

describe('relink/cases.json и relink/notes/', () => {
  const { cases } = load('relink/cases.json');
  const dir = join(ROOT, 'relink');
  const bytesOf = (file: string): Buffer => readFileSync(join(dir, file));

  it.each(cases.map((c, i) => [str(c.name, 'name'), i, c]))('%s', (name, i, c) => {
    const where = `relink/cases.json #${i} ${name}`;
    keys(c, ['name', 'note', 'spec', 'vault', 'op', 'file', 'before', 'after', 'features'], where);
    str(c.note, 'note');
    str(c.spec, 'spec');
    const vault = arr(c.vault, `${where}.vault`).map((v, k) => {
      const note = obj(v, `${where}.vault[${k}]`);
      keys(note, ['path', 'created'], `${where}.vault[${k}]`);
      int(note.created, 'created');
      const path = str(note.path, 'path');
      expect(path, 'путь заметки от корня через /').toMatch(/^[^/\\].*\.md$/u);
      return path;
    });
    expect(new Set(vault).size, 'пути не повторяются').toBe(vault.length);
    const op = obj(c.op, `${where}.op`);
    keys(op, ['entry', 'from', 'to'], `${where}.op`);
    const from = str(op.from, 'op.from');
    str(op.to, 'op.to');
    const file = str(c.file, 'file');
    expect(vault).toContain(file);
    if (op.entry === 'note') {
      expect(vault, 'переименуемая заметка есть в хранилище').toContain(from);
      expect(file).not.toBe(from);
    } else {
      expect(op.entry).toBe('folder');
      expect(vault.some((p) => p.startsWith(`${from}/`)), 'в папке есть заметки').toBe(true);
      expect(file.startsWith(`${from}/`)).toBe(false);
    }
    const before = bytesOf(str(c.before, 'before'));
    const after = bytesOf(str(c.after, 'after'));
    const features = arr(c.features, `${where}.features`).map((f) => str(f, 'feature'));
    const unchanged = c.after === c.before;
    expect(features.includes('unchanged'), 'unchanged ⇔ after = before').toBe(unchanged);
    if (!unchanged) expect(before.equals(after), 'after отличается от before').toBe(false);
    for (const bytes of [before, after]) {
      const text = bytes.toString('latin1');
      const crlf = (text.match(/\r\n/gu) ?? []).length;
      const lf = (text.match(/(?<!\r)\n/gu) ?? []).length;
      for (const feature of features) {
        switch (feature) {
          case 'bom':
            expect(bytes.subarray(0, 3).equals(Buffer.from([0xef, 0xbb, 0xbf])), `${where}: BOM`).toBe(true);
            break;
          case 'crlf':
            expect(crlf > 0 && lf === 0, `${where}: все переносы CRLF (git не нормализовал)`).toBe(true);
            break;
          case 'lf':
            expect(lf > 0 && crlf === 0, `${where}: переносы только LF`).toBe(true);
            break;
          case 'mixed_eol':
            expect(crlf > 0 && lf > 0, `${where}: смешанные переносы`).toBe(true);
            break;
          case 'trailing_spaces':
            expect(/[ \t]\r?\n/u.test(text), `${where}: концевые пробелы`).toBe(true);
            break;
          case 'no_final_newline':
            expect(bytes.at(-1), `${where}: без перевода строки в конце`).not.toBe(0x0a);
            break;
          case 'code':
          case 'frontmatter':
          case 'unchanged':
            break;
          default:
            throw new Error(`${where}: неизвестная особенность ${feature}`);
        }
      }
      if (!features.includes('bom')) expect(bytes[0], `${where}: без BOM`).not.toBe(0xef);
    }
  });

  it('в relink/notes/ нет файлов без образца', () => {
    const used = new Set(cases.flatMap((c) => [c.before, c.after]));
    const orphans = readdirSync(join(dir, 'notes')).filter((f) => !used.has(`notes/${f}`));
    expect(orphans).toEqual([]);
  });
});
