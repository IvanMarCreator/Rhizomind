// Разобранные исключения (решение владельца 52): находка в собранном интерфейсе
// проходит, только если карта исходников ведёт в пакет из node_modules и в
// списке есть запись с той же версией, файлом, строкой, хэшем строки и видом.
import { mkdirSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { ALLOWED_KINDS, checkNetwork, fileSha256, lineSha256, parseAllowlist, type Exception } from './allowlist.ts';

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
  /** Папка пакета, если не node_modules/<имя> (вложенная копия). */
  dir?: string;
  /** Поля записи package-lock.json поверх { version, integrity }; undefined — убрать поле. */
  lock?: Record<string, unknown>;
  files: Record<string, string>;
}

const WINDOW = 'глобальный объект window без свойства';

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
  const lock: Record<string, Record<string, unknown>> = {};
  for (const pkg of packages) {
    const dir = pkg.dir ?? `node_modules/${pkg.name}`;
    lock[dir] = { version: pkg.version, integrity: 'sha512-dGVzdA==', ...pkg.lock };
    write(`${dir}/package.json`, JSON.stringify({ name: pkg.name, version: pkg.installed ?? pkg.version }));
    for (const [rel, text] of Object.entries(pkg.files)) write(`${dir}/${rel}`, text);
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

interface RealOptions {
  /** Правка строки записи (для выбранного пакета). */
  edit?: (r: Exception) => ((code: string) => string) | undefined;
  /** Правка всего файла пакета после того, как записи разобраны. */
  editFile?: (r: Exception) => ((text: string) => string) | undefined;
  version?: string;
  /** Положить пакеты внутрь другого пакета: <nested>/node_modules/<имя>. */
  nested?: string;
}

/**
 * Проект из настоящего списка: пакеты и файлы записей с теми же версиями и
 * строками на тех же номерах. Записи — настоящие, но fileSha256 — от файла
 * тестового пакета (настоящий файл сверяет сам `npm run check:network`).
 */
function realProject(options: RealOptions = {}): { p: Project; records: Exception[] } {
  const byFile = new Map<string, Exception[]>();
  for (const r of REAL) {
    const key = `${r.package}\u0000${r.file}`;
    byFile.set(key, [...(byFile.get(key) ?? []), r]);
  }
  const packages = new Map<string, Package>();
  const bundle: string[] = [];
  const records: Exception[] = [];
  for (const group of byFile.values()) {
    const { package: name, version, file } = group[0] as Exception;
    const dir = options.nested ? `${options.nested}/node_modules/${name}` : `node_modules/${name}`;
    const pkg = packages.get(name) ?? { name, dir, version: options.version ?? version, files: {} };
    const reviewed = packageFile(group);
    records.push(...group.map((r) => ({ ...r, fileSha256: fileSha256(reviewed) })));
    const text = packageFile(group, options.edit?.(group[0] as Exception));
    pkg.files[file] = (options.editFile?.(group[0] as Exception) ?? ((t: string) => t))(text);
    packages.set(name, pkg);
    bundle.push(`${dir}/${file}`);
  }
  return { p: project([...packages.values()], {}, bundle), records };
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
  package: 'lib',
  version: '1.0.0',
  file: 'index.js',
  fileSha256: '0'.repeat(64),
  line: 1,
  kind: WINDOW,
  count: 1,
  code: '',
  lineSha256: lineSha256(''),
  reason: 'тест',
  reviewedBy: 'тест',
  reviewedOn: '2026-10-26',
  ...over,
});

/** Пакет lib@1.0.0 из одной строки и запись, точно ей соответствующая. */
function libProject(code: string, over: Partial<Package> = {}, kind = WINDOW) {
  const text = `${code}\n`;
  const p = project([{ name: 'lib', version: '1.0.0', files: { 'index.js': text }, ...over }], {}, [
    `${over.dir ?? 'node_modules/lib'}/index.js`,
  ]);
  return { p, rec: record({ kind, code, lineSha256: lineSha256(code), fileSha256: fileSha256(text) }) };
}

const notUnused = (m: string) => !m.startsWith('Неиспользованное');

describe('список исключений: настоящий файл', () => {
  it('9 разобранных находок; у каждой причина, хэш строки, хэш установленного файла, count 1 и разрешимый вид', () => {
    expect(REAL).toHaveLength(9);
    for (const r of REAL) {
      expect(r.reason.length).toBeGreaterThan(20);
      expect(r.lineSha256).toBe(lineSha256(r.code));
      expect(r.count).toBe(1);
      expect(ALLOWED_KINDS.has(r.kind)).toBe(true);
      const installed = readFileSync(new URL(`../../node_modules/${r.package}/${r.file}`, import.meta.url));
      expect(r.fileSha256).toBe(fileSha256(installed));
    }
  });
});

describe('checkNetwork: находки в чужом пакете', () => {
  it('все 9 находок со своими записями проходят, и каждая запись использована', async () => {
    const { p, records } = realProject();
    const result = await run(p, records);
    expect(result.problems).toEqual([]);
    expect(result.allowed).toHaveLength(9);
  });

  it.each(REAL.map((r, i) => [`${r.package} ${r.file}:${r.line} [${r.kind}]`, i] as const))(
    'без записи %s — красное',
    async (_name, index) => {
      const { p, records } = realProject();
      const result = await run(p, records.filter((_, i) => i !== index));
      const r = REAL[index] as Exception;
      expect(result.problems).toHaveLength(1);
      expect(result.problems[0]).toContain(`${r.package}@${r.version} ${r.file}:${r.line}`);
      expect(result.problems[0]).toContain(r.kind);
      expect(result.problems[0]).toContain('исключения нет');
    },
  );

  it('изменилась строка в пакете — исключение недействительно: «разберите заново»', async () => {
    const target = REAL.find((r) => r.package === '@codemirror/commands') as Exception;
    const { p, records } = realProject({ edit: (r) => (r.package === target.package ? (c) => `${c} // правка` : undefined) });
    const result = await run(p, records);
    expect(result.problems.some((m) => m.includes(`${target.package}@${target.version} ${target.file}:${target.line}`) && m.includes('разберите заново'))).toBe(true);
    expect(result.problems.some((m) => m.startsWith('Неиспользованное исключение') && m.includes(target.package))).toBe(true);
  });

  it('файл пакета изменён вне разрешённых строк при той же версии — все его исключения недействительны (ревью, Б1)', async () => {
    const { p, records } = realProject({
      editFile: (r) => (r.package === '@codemirror/view' ? (t) => `${t}fetch("https://x.test");\n` : undefined),
    });
    const result = await run(p, records);
    const view = records.filter((r) => r.package === '@codemirror/view');
    expect(result.allowed.filter((a) => a.origin.package === '@codemirror/view')).toEqual([]);
    expect(result.problems.filter((m) => notUnused(m) && m.includes('@codemirror/view') && m.includes('разберите заново'))).toHaveLength(view.length);
    expect(result.problems.some((m) => m.includes('fetch') && m.includes('сетевой вызов'))).toBe(true);
  });

  it('сменилась версия пакета — все исключения недействительны', async () => {
    const { p, records } = realProject({ version: '99.0.0' });
    const result = await run(p, records);
    expect(result.allowed).toEqual([]);
    expect(result.problems.filter((m) => notUnused(m) && m.includes('разберите заново'))).toHaveLength(9);
    expect(result.problems.filter((m) => m.startsWith('Неиспользованное исключение'))).toHaveLength(9);
  });

  it('вложенная копия разрешённого пакета той же версии — исключения не действуют (ревью, Б1)', async () => {
    const { p, records } = realProject({ nested: 'node_modules/evilpkg' });
    const result = await run(p, records);
    expect(result.allowed).toEqual([]);
    expect(result.problems.filter((m) => m.includes('вложенная копия пакета'))).toHaveLength(9);
  });

  it('в package-lock.json нет integrity — красное', async () => {
    const { p, rec } = libProject('const w = d || window;', { lock: { integrity: undefined } });
    const result = await run(p, [rec]);
    expect(result.allowed).toEqual([]);
    expect(result.problems.some((m) => m.includes('integrity'))).toBe(true);
  });

  it('пакет в package-lock.json с inBundle — красное', async () => {
    const { p, rec } = libProject('const w = d || window;', { lock: { inBundle: true } });
    const result = await run(p, [rec]);
    expect(result.allowed).toEqual([]);
    expect(result.problems.some((m) => m.includes('inBundle'))).toBe(true);
  });

  it('установленная версия расходится с package-lock.json — красное', async () => {
    const { p, rec } = libProject('const w = d || window;', { installed: '1.0.1' });
    const result = await run(p, [rec]);
    expect(result.problems.some((m) => m.includes('package-lock.json'))).toBe(true);
    expect(result.allowed).toEqual([]);
  });

  it('запись с точным соответствием проходит', async () => {
    const { p, rec } = libProject('const w = d || window;');
    expect((await run(p, [rec])).problems).toEqual([]);
  });

  it('count: две находки в строке, а разобрана одна — красное; count 2 — проходит', async () => {
    const { p, rec } = libProject('const a = x || window, b = y || window;');
    const one = await run(p, [rec]);
    expect(one.problems).toEqual([expect.stringContaining('покрывает находок: 2, а разобрано (count): 1')]);
    expect((await run(p, [{ ...rec, count: 2 }])).problems).toEqual([]);
  });

  it('count больше, чем находок, — красное', async () => {
    const { p, rec } = libProject('const w = d || window;');
    const result = await run(p, [{ ...rec, count: 2 }]);
    expect(result.problems).toEqual([expect.stringContaining('покрывает находок: 1, а разобрано (count): 2')]);
  });

  it('по карте в исходнике на этом месте нет такой находки — красное', async () => {
    const { p, rec } = libProject('const ww = d || window;');
    // Сборка отличается от исходника на один знак: карта «символ в символ»
    // указывает в исходнике на «|», а не на window.
    for (const dir of [p.dist, p.maps]) writeFileSync(path.join(dir, 'assets/index.js'), 'const w = d || window;\n');
    const result = await run(p, [rec]);
    expect(result.allowed).toEqual([]);
    expect(result.problems.some((m) => m.includes('в исходнике на этом месте нет такой находки'))).toBe(true);
  });

  it('неиспользованная запись — ошибка', async () => {
    const { p, records } = realProject();
    const extra = record({ package: 'style-mod', version: '4.1.4', file: 'src/style-mod.js', line: 99 });
    const result = await run(p, [...records, extra]);
    expect(result.problems).toEqual([expect.stringMatching(/^Неиспользованное исключение: style-mod@4\.1\.4 src\/style-mod\.js:99/)]);
  });
});

// Сеть и виды находок, которые исключением не разрешаются (ревью, Б2).
const FORBIDDEN = [
  ['fetch(u);', 'fetch'],
  ['new XMLHttpRequest();', 'XMLHttpRequest'],
  ['new WebSocket(u);', 'WebSocket'],
  ['navigator.sendBeacon(u, d);', 'sendBeacon'],
  ['new EventSource(u);', 'EventSource'],
  ['window.open(u);', 'window.open'],
  ['location.href = u;', 'присваивание location'],
  ['new Image().src = u;', 'Image'],
  ['new Audio(u);', 'Audio'],
  ['import(u);', 'import(<вычисляемое>)'],
  ['require(u);', 'require(<вычисляемое>)'],
  ['eval(c);', 'eval'],
  ['new Function(c);', 'Function'],
  ['setTimeout("f()", 1);', 'setTimeout(<строка>)'],
  ['document.createElement("script");', 'createElement(script)'],
  ['document.createElement("iframe");', 'createElement(iframe)'],
  ['document.createElement("link");', 'createElement(link)'],
  ['document.createElement("form");', 'createElement(form)'],
  ['window[k](u);', 'window[<вычисляемое>]'],
  ['navigator[k](u);', 'navigator[<вычисляемое>]'],
  ['view.win[k](u);', 'view.win[<вычисляемое>]'],
] as const;

describe('checkNetwork: сеть и неразрешимые виды', () => {
  it.each(FORBIDDEN)('%s в чужом пакете без записи — красное', async (code, kind) => {
    const { p } = libProject(code);
    const result = await run(p, []);
    expect(result.problems).toHaveLength(1);
    expect(result.problems[0]).toContain(kind);
    expect(result.problems[0]).toContain('lib@1.0.0 index.js:1');
  });

  it.each(FORBIDDEN)('%s — даже с точной записью красное', async (code, kind) => {
    const { p, rec } = libProject(code, {}, kind);
    const result = await run(p, [rec]);
    expect(result.allowed).toEqual([]);
    expect(result.problems.some((m) => m.includes(kind) && /сетевой вызов|исключением не разрешается/.test(m))).toBe(true);
    expect(result.problems.some((m) => m.startsWith('Неиспользованное исключение'))).toBe(true);
  });

  it.each(FORBIDDEN)('parseAllowlist не принимает запись вида находки от %s', (code, kind) => {
    expect(() => parseAllowlist({ exceptions: [record({ kind, code, lineSha256: lineSha256(code) })] })).toThrow(
      /исключением не разрешается/,
    );
  });

  it('parseAllowlist не принимает «Function» без instanceof, но принимает «Function справа от instanceof»', () => {
    expect(() => parseAllowlist({ exceptions: [record({ kind: 'Function' })] })).toThrow(/исключением не разрешается/);
    expect(parseAllowlist({ exceptions: [record({ kind: 'Function справа от instanceof' })] })).toHaveLength(1);
  });
});

describe('checkNetwork: наш код не разрешается', () => {
  it('запись не разрешает находку в нашем коде: ни в src/, ни в сборке из src/', async () => {
    const code = 'const w = d || window;';
    const p = project([], { 'src/main.ts': `${code}\n` }, ['src/main.ts']);
    const result = await run(
      p,
      [record({ package: 'src', file: 'main.ts', code, lineSha256: lineSha256(code), fileSha256: fileSha256(`${code}\n`) })],
      ['src', 'dist'],
    );
    expect(result.allowed).toEqual([]);
    expect(result.problems.filter((m) => notUnused(m) && m.includes('глобальный объект window'))).toHaveLength(2);
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
      JSON.stringify({ lockfileVersion: 3, packages: { 'node_modules/fake': { version: '1.0.0', integrity: 'sha512-dGVzdA==' } } }),
    );
    // Сборка из файла через ссылку: источник в карте — node_modules/fake/main.ts.
    writeBundle(p.base, ['node_modules/fake/main.ts']);
    const result = await run(p, [
      record({ package: 'fake', file: 'main.ts', code, lineSha256: lineSha256(code), fileSha256: fileSha256(`${code}\n`) }),
    ]);
    expect(result.allowed).toEqual([]);
    expect(result.problems.some((m) => m.includes('наш код'))).toBe(true);
  });

  it('карта исходников без точного соответствия позиции — красное', async () => {
    const { p } = libProject('const w = d || window;');
    writeFileSync(path.join(p.maps, 'assets/index.js.map'), JSON.stringify({ version: 3, sources: [], names: [], mappings: '' }));
    const result = await run(p, []);
    expect(result.problems).toEqual([expect.stringContaining('источник не найден по карте')]);
  });
});

describe('checkNetwork: копия сборки с картами', () => {
  it('копия с картами отличается от проверяемой сборки — красное', async () => {
    const { p, records } = realProject();
    writeFileSync(path.join(p.maps, 'assets/index.js'), '// другая сборка\n');
    const result = await run(p, records);
    expect(result.problems.some((m) => m.includes('отличается'))).toBe(true);
  });

  it('файла нет в копии с картами — красное', async () => {
    const { p, records } = realProject();
    writeFileSync(path.join(p.dist, 'assets/extra.js'), 'const a = 1;\n');
    const result = await run(p, records);
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
    ['нет поля fileSha256', { exceptions: [{ ...valid.exceptions[0], fileSha256: undefined }] }],
    ['нет поля count', { exceptions: [{ ...valid.exceptions[0], count: undefined }] }],
    ['count 0', { exceptions: [{ ...valid.exceptions[0], count: 0 }] }],
    ['fileSha256 не хэш', { exceptions: [{ ...valid.exceptions[0], fileSha256: 'abc' }] }],
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
