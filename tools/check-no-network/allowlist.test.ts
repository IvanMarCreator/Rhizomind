// Разобранные исключения (решение владельца 52): находка в собранном интерфейсе
// проходит, только если карта исходников ведёт в пакет из node_modules и в
// списке есть запись с той же версией, файлом, строкой, хэшем строки и видом.
import { mkdirSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { checkNetwork, lineSha256, parseAllowlist, type Exception } from './allowlist.ts';

const REAL = parseAllowlist(JSON.parse(readFileSync(new URL('./allowlist.json', import.meta.url), 'utf8')));

// --- Сборка тестового проекта: пакеты, исходники, dist и копия dist с картой ---

const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
function vlq(value: number): string {
  let rest = value < 0 ? (-value << 1) | 1 : value << 1;
  let out = '';
  do {
    let digit = rest & 31;
    rest >>>= 5;
    if (rest > 0) digit |= 32;
    out += B64[digit];
  } while (rest > 0);
  return out;
}

interface Project {
  base: string;
  dist: string;
  maps: string;
}

interface Package {
  name: string;
  version: string;
  /** Версия в package.json установленного пакета, если отличается от package-lock.json. */
  installed?: string;
  files: Record<string, string>;
}

/**
 * Проект: пакеты в node_modules, свои файлы, dist/assets/index.js — склейка
 * перечисленных исходников, и копия dist с картой исходников (каждый символ
 * сборки указывает на тот же символ исходника).
 */
function project(packages: Package[], own: Record<string, string>, bundle: string[]): Project {
  const base = mkdtempSync(path.join(tmpdir(), 'check-allowlist-'));
  const write = (rel: string, text: string) => {
    mkdirSync(path.dirname(path.join(base, rel)), { recursive: true });
    writeFileSync(path.join(base, rel), text);
  };
  const lock: Record<string, { version: string }> = {};
  for (const pkg of packages) {
    lock[`node_modules/${pkg.name}`] = { version: pkg.version };
    write(`node_modules/${pkg.name}/package.json`, JSON.stringify({ name: pkg.name, version: pkg.installed ?? pkg.version }));
    for (const [rel, text] of Object.entries(pkg.files)) write(`node_modules/${pkg.name}/${rel}`, text);
  }
  write('package-lock.json', JSON.stringify({ lockfileVersion: 3, packages: { '': {}, ...lock } }));
  for (const [rel, text] of Object.entries(own)) write(rel, text);

  writeBundle(base, bundle);
  return { base, dist: path.join(base, 'dist'), maps: path.join(base, '.network-check/dist') };
}

/** dist/assets/index.js — склейка исходников; копия в .network-check/dist с картой «символ в символ». */
function writeBundle(base: string, bundle: string[]): void {
  const mapsAssets = path.join(base, '.network-check/dist/assets');
  const lines: string[] = [];
  const segmentsByLine: string[] = [];
  const state = { source: 0, line: 0, column: 0 };
  bundle.forEach((rel, source) => {
    const text = readFileSync(path.join(base, rel), 'utf8').replace(/\n$/, '');
    text.split('\n').forEach((line, origLine) => {
      const segments: string[] = [];
      let genColumn = 0;
      for (let column = 0; column < line.length; column++) {
        segments.push(
          vlq(column - genColumn) + vlq(source - state.source) + vlq(origLine - state.line) + vlq(column - state.column),
        );
        genColumn = column;
        Object.assign(state, { source, line: origLine, column });
      }
      segmentsByLine.push(segments.join(','));
      lines.push(line);
    });
  });
  const js = lines.join('\n') + '\n';
  const html = '<script type="module" src="./assets/index.js"></script>\n';
  for (const dir of ['dist', '.network-check/dist']) {
    mkdirSync(path.join(base, dir, 'assets'), { recursive: true });
    writeFileSync(path.join(base, dir, 'assets/index.js'), js);
    writeFileSync(path.join(base, dir, 'index.html'), html);
  }
  const sources = bundle.map((rel) => path.relative(mapsAssets, path.join(base, rel)).split(path.sep).join('/'));
  writeFileSync(
    path.join(mapsAssets, 'index.js.map'),
    JSON.stringify({ version: 3, sources, names: [], mappings: segmentsByLine.join(';') }),
  );
}

/** Файл пакета: строки записей стоят на своих номерах, остальное — комментарии. */
function packageFile(records: Exception[], edit: (code: string) => string = (c) => c): string {
  const last = Math.max(...records.map((r) => r.line)) + 1;
  const lines = Array.from({ length: last }, () => '// -');
  for (const r of records) {
    lines[r.line - 1] = edit(r.code);
    // Геттер класса не разбирается отдельно — оборачиваем в класс.
    if (r.code.trim().startsWith('get ')) {
      lines[r.line - 2] = 'class V {';
      lines[r.line] = '}';
    }
  }
  return lines.join('\n') + '\n';
}

/** Проект из настоящего списка: каждый пакет и файл записей с теми же версиями и строками. */
function realProject(options: { edit?: (r: Exception) => ((code: string) => string) | undefined; version?: string } = {}) {
  const byFile = new Map<string, Exception[]>();
  for (const r of REAL) {
    const key = `${r.package}\u0000${r.file}`;
    byFile.set(key, [...(byFile.get(key) ?? []), r]);
  }
  const packages = new Map<string, Package>();
  const bundle: string[] = [];
  for (const records of byFile.values()) {
    const { package: name, version, file } = records[0] as Exception;
    const pkg = packages.get(name) ?? { name, version: options.version ?? version, files: {} };
    const edit = options.edit?.(records[0] as Exception);
    pkg.files[file] = packageFile(records, edit);
    packages.set(name, pkg);
    bundle.push(`node_modules/${name}/${file}`);
  }
  return project([...packages.values()], {}, bundle);
}

async function run(p: Project, allowlist: Exception[], targets = ['dist']) {
  return checkNetwork({
    targets: targets.map((t) => path.join(p.base, t)),
    base: p.base,
    maps: [{ dist: p.dist, maps: p.maps }],
    allowlist,
  });
}

const record = (over: Partial<Exception>): Exception => ({
  package: 'evil',
  version: '1.0.0',
  file: 'index.js',
  line: 1,
  kind: 'fetch',
  code: '',
  lineSha256: lineSha256(''),
  reason: 'тест',
  reviewedBy: 'тест',
  reviewedOn: '2026-10-26',
  ...over,
});

describe('список исключений: настоящий файл', () => {
  it('в списке 9 разобранных находок CodeMirror и style-mod, у каждой есть причина и хэш своей строки', () => {
    expect(REAL).toHaveLength(9);
    for (const r of REAL) {
      expect(r.reason.length).toBeGreaterThan(20);
      expect(r.lineSha256).toBe(lineSha256(r.code));
    }
  });
});

describe('checkNetwork: находки в чужом пакете', () => {
  it('все 9 находок со своими записями проходят, и каждая запись использована', async () => {
    const result = await run(realProject(), REAL);
    expect(result.problems).toEqual([]);
    expect(result.allowed).toHaveLength(9);
  });

  it.each(REAL.map((r, i) => [`${r.package} ${r.file}:${r.line} [${r.kind}]`, i] as const))(
    'без записи %s — красное',
    async (_name, index) => {
      const result = await run(realProject(), REAL.filter((_, i) => i !== index));
      const r = REAL[index] as Exception;
      expect(result.problems).toHaveLength(1);
      expect(result.problems[0]).toContain(`${r.package}@${r.version} ${r.file}:${r.line}`);
      expect(result.problems[0]).toContain(r.kind);
      expect(result.problems[0]).toContain('исключения нет');
    },
  );

  it('изменилась строка в пакете — исключение недействительно: «разберите заново»', async () => {
    const target = REAL.find((r) => r.package === '@codemirror/commands') as Exception;
    const p = realProject({ edit: (r) => (r.package === target.package ? (c) => `${c} // правка` : undefined) });
    const result = await run(p, REAL);
    expect(result.problems.some((m) => m.includes(`${target.package}@${target.version} ${target.file}:${target.line}`) && m.includes('разберите заново'))).toBe(true);
    expect(result.problems.some((m) => m.startsWith('Неиспользованное исключение') && m.includes(target.package))).toBe(true);
  });

  it('сменилась версия пакета — все исключения недействительны', async () => {
    const result = await run(realProject({ version: '99.0.0' }), REAL);
    expect(result.allowed).toEqual([]);
    expect(result.problems.filter((m) => !m.startsWith('Неиспользованное') && m.includes('разберите заново'))).toHaveLength(9);
    expect(result.problems.filter((m) => m.startsWith('Неиспользованное исключение'))).toHaveLength(9);
  });

  it('установленная версия расходится с package-lock.json — красное', async () => {
    const p = project(
      [{ name: 'lib', version: '1.0.0', installed: '1.0.1', files: { 'index.js': 'const w = d || window;\n' } }],
      {},
      ['node_modules/lib/index.js'],
    );
    const code = 'const w = d || window;';
    const result = await run(p, [
      record({ package: 'lib', kind: 'глобальный объект window без свойства', code, lineSha256: lineSha256(code) }),
    ]);
    expect(result.problems.some((m) => m.includes('package-lock.json'))).toBe(true);
    expect(result.allowed).toEqual([]);
  });

  it('неиспользованная запись — ошибка', async () => {
    const extra = record({ package: 'style-mod', version: '4.1.4', file: 'src/style-mod.js', line: 99 });
    const result = await run(realProject(), [...REAL, extra]);
    expect(result.problems).toEqual([expect.stringMatching(/^Неиспользованное исключение: style-mod@4\.1\.4 src\/style-mod\.js:99/)]);
  });
});

describe('checkNetwork: сетевые вызовы и наш код не разрешаются', () => {
  const calls = [
    ['fetch(u);', 'fetch'],
    ['new XMLHttpRequest();', 'XMLHttpRequest'],
    ['new WebSocket(u);', 'WebSocket'],
    ['navigator.sendBeacon(u, d);', 'sendBeacon'],
    ['new EventSource(u);', 'EventSource'],
    ['new Image().src = u;', 'Image'],
    ['window.open(u);', 'window.open'],
    ['location.href = u;', 'присваивание location'],
  ] as const;

  it.each(calls)('%s в чужом пакете без записи — красное', async (code, kind) => {
    const p = project([{ name: 'evil', version: '1.0.0', files: { 'index.js': `${code}\n` } }], {}, ['node_modules/evil/index.js']);
    const result = await run(p, []);
    expect(result.problems).toHaveLength(1);
    expect(result.problems[0]).toContain(kind);
    expect(result.problems[0]).toContain('evil@1.0.0 index.js:1');
  });

  it.each(calls.filter(([, kind]) => kind !== 'Image'))(
    '%s — сетевой вызов, запись его не разрешает',
    async (code, kind) => {
      const p = project([{ name: 'evil', version: '1.0.0', files: { 'index.js': `${code}\n` } }], {}, ['node_modules/evil/index.js']);
      const result = await run(p, [record({ kind, code, lineSha256: lineSha256(code) })]);
      expect(result.allowed).toEqual([]);
      expect(result.problems.some((m) => m.includes('сетевой вызов'))).toBe(true);
      expect(result.problems.some((m) => m.startsWith('Неиспользованное исключение'))).toBe(true);
    },
  );

  it('запись не разрешает находку в нашем коде: ни в src/, ни в сборке из src/', async () => {
    const code = 'const w = d || window;';
    const p = project([], { 'src/main.ts': `${code}\n` }, ['src/main.ts']);
    const result = await run(
      p,
      [record({ package: 'src', file: 'main.ts', kind: 'глобальный объект window без свойства', code, lineSha256: lineSha256(code) })],
      ['src', 'dist'],
    );
    expect(result.allowed).toEqual([]);
    expect(result.problems.filter((m) => !m.startsWith('Неиспользованное') && m.includes('глобальный объект window'))).toHaveLength(2);
    expect(result.problems.some((m) => m.includes('dist/assets/index.js') && m.includes('наш код'))).toBe(true);
  });

  it('пакет-ссылка на папку src/ (junction) — всё равно наш код', async () => {
    const code = 'const w = d || window;';
    const p = project([], { 'src/main.ts': `${code}\n` }, []);
    mkdirSync(path.join(p.base, 'node_modules'), { recursive: true });
    symlinkSync(path.join(p.base, 'src'), path.join(p.base, 'node_modules/fake'), 'junction');
    writeFileSync(path.join(p.base, 'src/package.json'), JSON.stringify({ name: 'fake', version: '1.0.0' }));
    writeFileSync(
      path.join(p.base, 'package-lock.json'),
      JSON.stringify({ lockfileVersion: 3, packages: { 'node_modules/fake': { version: '1.0.0' } } }),
    );
    // Сборка из файла через ссылку: источник в карте — node_modules/fake/main.ts.
    writeBundle(p.base, ['node_modules/fake/main.ts']);
    const result = await run(p, [
      record({ package: 'fake', file: 'main.ts', kind: 'глобальный объект window без свойства', code, lineSha256: lineSha256(code) }),
    ]);
    expect(result.allowed).toEqual([]);
    expect(result.problems.some((m) => m.includes('наш код'))).toBe(true);
  });

  it('карта исходников без точного соответствия позиции — красное', async () => {
    const p = project([{ name: 'lib', version: '1.0.0', files: { 'index.js': 'const w = d || window;\n' } }], {}, ['node_modules/lib/index.js']);
    writeFileSync(path.join(p.maps, 'assets/index.js.map'), JSON.stringify({ version: 3, sources: [], names: [], mappings: '' }));
    const result = await run(p, []);
    expect(result.problems).toEqual([expect.stringContaining('источник не найден по карте')]);
  });
});

describe('checkNetwork: копия сборки с картами', () => {
  it('копия с картами отличается от проверяемой сборки — красное', async () => {
    const p = realProject();
    writeFileSync(path.join(p.maps, 'assets/index.js'), '// другая сборка\n');
    const result = await run(p, REAL);
    expect(result.problems.some((m) => m.includes('отличается'))).toBe(true);
  });

  it('файла нет в копии с картами — красное', async () => {
    const p = realProject();
    writeFileSync(path.join(p.dist, 'assets/extra.js'), 'const a = 1;\n');
    const result = await run(p, REAL);
    expect(result.problems.some((m) => m.includes('assets/extra.js'))).toBe(true);
  });
});

describe('parseAllowlist', () => {
  const valid = { exceptions: [record({ code: 'x', lineSha256: lineSha256('x') })] };

  it('принимает правильный список', () => {
    expect(parseAllowlist(valid)).toHaveLength(1);
  });

  it.each([
    ['нет поля reason', { exceptions: [{ ...valid.exceptions[0], reason: undefined }] }],
    ['лишнее поле', { exceptions: [{ ...valid.exceptions[0], extra: 1 }] }],
    ['хэш не совпадает с code', { exceptions: [{ ...valid.exceptions[0], lineSha256: lineSha256('y') }] }],
    ['файл выходит из пакета', { exceptions: [{ ...valid.exceptions[0], file: '../../src/main.ts' }] }],
    ['абсолютный путь файла', { exceptions: [{ ...valid.exceptions[0], file: '/src/main.ts' }] }],
    ['имя пакета с ..', { exceptions: [{ ...valid.exceptions[0], package: '../src' }] }],
    ['дата не ГГГГ-ММ-ДД', { exceptions: [{ ...valid.exceptions[0], reviewedOn: 'вчера' }] }],
    ['нет списка', {}],
  ])('отклоняет: %s', (_name, json) => {
    expect(() => parseAllowlist(json)).toThrow();
  });
});
