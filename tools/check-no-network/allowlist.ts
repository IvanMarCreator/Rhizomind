// Разобранные исключения проверки сети (решение владельца 52).
//
// Находка в собранном интерфейсе (dist/) проходит, только если:
// 1) это не сетевой вызов (severity «подозрительно»);
// 2) карта исходников копии сборки точно (та же строка и позиция) ведёт к файлу
//    пакета в node_modules — по настоящему пути, без ссылок в наш код;
//    пакета верхнего уровня (`node_modules/<имя>`, в package-lock.json с
//    `integrity` и без `inBundle`/`link`), и в исходнике на этом месте — та же
//    находка (исходник разбирается заново);
// 3) вид находки — из закрытого списка ALLOWED_KINDS;
// 4) в списке есть запись, у которой совпадают все поля: пакет, версия из
//    package-lock.json, файл в пакете, SHA-256 всего файла, номер строки,
//    SHA-256 строки и вид находки, — и запись покрывает ровно `count` находок.
// Находки в src/ и src-isolation/ и в сборке из них исключениями не разрешаются.
// Неиспользованная запись — тоже ошибка: список не копит мусор.
//
// Карты исходников в выпуск не попадают: проверка собирает копию интерфейса с
// картами (`vite build --sourcemap hidden` в .network-check/dist) и требует,
// чтобы каждый файл dist/ совпадал с копией байт в байт.
import { createHash } from 'node:crypto';
import { readdir, readFile, realpath } from 'node:fs/promises';
import { SourceMap, type SourceMapPayload } from 'node:module';
import path from 'node:path';
import { scanPaths, scanSourceLocated, type FileFinding, type LocatedFinding } from './scan.ts';

/**
 * Виды находок, которые можно разрешить исключением. Всё остальное (сеть,
 * `eval`/`Function` вне `instanceof`, `import()`/`require()` с вычисляемым
 * именем, `Image`/`Audio`, таймеры со строкой, `createElement` опасных
 * элементов, обращения по вычисляемому имени к window/navigator и т. д.) — нет.
 */
export const ALLOWED_KINDS: ReadonlySet<string> = new Set([
  'глобальный объект window без свойства',
  'глобальный объект globalThis без свойства',
  'Function справа от instanceof',
]);

export interface Exception {
  package: string;
  version: string;
  /** Путь файла внутри пакета, через «/». */
  file: string;
  /** SHA-256 всего файла пакета: правка в любом месте файла делает запись недействительной. */
  fileSha256: string;
  line: number;
  kind: string;
  /** Сколько находок этого вида в этой строке покрывает запись. */
  count: number;
  /** Текст исходной строки, который разобран (для чтения человеком). */
  code: string;
  /** SHA-256 строки без перевода строки, шестнадцатеричный. */
  lineSha256: string;
  reason: string;
  reviewedBy: string;
  reviewedOn: string;
}

export interface MapPair {
  /** Проверяемая сборка (то, что уходит в выпуск). */
  dist: string;
  /** Та же сборка с картами исходников. */
  maps: string;
}

export interface Origin {
  package: string;
  version: string;
  file: string;
  fileSha256: string;
  line: number;
  lineSha256: string;
}

export interface CheckResult {
  problems: string[];
  allowed: { finding: FileFinding; origin: Origin; exception: Exception }[];
}

export function lineSha256(code: string): string {
  return createHash('sha256').update(code, 'utf8').digest('hex');
}

export function fileSha256(bytes: Buffer | string): string {
  return createHash('sha256').update(bytes).digest('hex');
}

const FIELDS: Record<keyof Exception, 'string' | 'number'> = {
  package: 'string',
  version: 'string',
  file: 'string',
  fileSha256: 'string',
  line: 'number',
  kind: 'string',
  count: 'number',
  code: 'string',
  lineSha256: 'string',
  reason: 'string',
  reviewedBy: 'string',
  reviewedOn: 'string',
};
const PACKAGE_NAME = /^(?:@[a-z0-9][\w.-]*\/)?[a-z0-9][\w.-]*$/i;

/** Разбор и проверка файла списка; ошибка — исключение с понятным текстом. */
export function parseAllowlist(json: unknown): Exception[] {
  const list = (json as { exceptions?: unknown } | null)?.exceptions;
  if (!Array.isArray(list)) throw new Error('В списке исключений нет массива "exceptions"');
  return list.map((item: unknown, index) => {
    const where = `запись ${index + 1} списка исключений`;
    if (typeof item !== 'object' || item === null) throw new Error(`${where}: не объект`);
    const record = item as Record<string, unknown>;
    for (const key of Object.keys(record)) {
      if (!(key in FIELDS)) throw new Error(`${where}: лишнее поле "${key}"`);
    }
    for (const [key, type] of Object.entries(FIELDS)) {
      if (typeof record[key] !== type) throw new Error(`${where}: нет поля "${key}" (${type})`);
    }
    const e = record as unknown as Exception;
    if (!PACKAGE_NAME.test(e.package) || e.package.split('/').some((part) => part === '.' || part === '..')) {
      throw new Error(`${where}: неправильное имя пакета "${e.package}"`);
    }
    const parts = e.file.split('/');
    if (e.file.startsWith('/') || e.file.includes('\\') || parts.some((p) => p === '' || p === '.' || p === '..')) {
      throw new Error(`${where}: путь файла должен быть внутри пакета, через «/»: "${e.file}"`);
    }
    if (!Number.isInteger(e.line) || e.line < 1) throw new Error(`${where}: неправильный номер строки`);
    if (!Number.isInteger(e.count) || e.count < 1) throw new Error(`${where}: count — целое число от 1`);
    if (!ALLOWED_KINDS.has(e.kind)) {
      throw new Error(`${where}: вид «${e.kind}» исключением не разрешается (допустимы: ${[...ALLOWED_KINDS].join('; ')})`);
    }
    if (!/^[0-9a-f]{64}$/.test(e.fileSha256)) throw new Error(`${where}: fileSha256 — SHA-256 файла, 64 шестнадцатеричных знака`);
    if (!/^[0-9a-f]{64}$/.test(e.lineSha256) || e.lineSha256 !== lineSha256(e.code)) {
      throw new Error(`${where}: lineSha256 не совпадает с SHA-256 поля code`);
    }
    if (!/^\d{4}-\d{2}-\d{2}$/.test(e.reviewedOn)) throw new Error(`${where}: reviewedOn — дата ГГГГ-ММ-ДД`);
    if (e.reason.trim() === '' || e.reviewedBy.trim() === '' || e.kind.trim() === '') {
      throw new Error(`${where}: пустые reason, reviewedBy или kind`);
    }
    return e;
  });
}

async function listFiles(root: string, rel = ''): Promise<string[]> {
  const out: string[] = [];
  const entries = await readdir(path.join(root, rel), { withFileTypes: true });
  for (const entry of entries) {
    const child = rel === '' ? entry.name : `${rel}/${entry.name}`;
    if (entry.isDirectory()) out.push(...(await listFiles(root, child)));
    else out.push(child);
  }
  return out.sort();
}

/** Каждый файл проверяемой сборки совпадает с копией с картами байт в байт. */
async function compareBuilds(pair: MapPair, base: string): Promise<string[]> {
  const problems: string[] = [];
  const show = (p: string) => path.relative(base, p).split(path.sep).join('/');
  const distFiles = await listFiles(pair.dist);
  const mapFiles = new Set(await listFiles(pair.maps));
  for (const rel of distFiles) {
    if (!mapFiles.has(rel)) {
      problems.push(`${show(pair.dist)}/${rel}: нет в копии сборки с картами (${show(pair.maps)}) — пересоберите проверку`);
      continue;
    }
    const [a, b] = await Promise.all([readFile(path.join(pair.dist, rel)), readFile(path.join(pair.maps, rel))]);
    if (!a.equals(b)) {
      problems.push(`${show(pair.dist)}/${rel}: отличается от копии сборки с картами (${show(pair.maps)}) — карта не подходит к этой сборке`);
    }
  }
  const distSet = new Set(distFiles);
  for (const rel of mapFiles) {
    if (!rel.endsWith('.map') && !distSet.has(rel)) {
      problems.push(`${show(pair.maps)}/${rel}: лишний файл в копии сборки с картами — пересоберите проверку`);
    }
  }
  return problems;
}

interface Lock {
  packages?: Record<string, { version?: string; integrity?: unknown; inBundle?: unknown; link?: unknown }>;
}

type Located = { origin: Origin } | { ours: string } | { problem: string };

class OriginFinder {
  private readonly maps = new Map<string, SourceMap | null>();
  private readonly sources = new Map<string, { bytes: Buffer; lines: string[]; findings: LocatedFinding[] | null }>();
  private readonly nodeModules: string;
  private readonly base: string;
  private readonly realBase: string;
  private readonly lock: Lock;
  private readonly pairs: MapPair[];

  constructor(base: string, realBase: string, lock: Lock, pairs: MapPair[]) {
    this.base = base;
    this.realBase = realBase;
    this.lock = lock;
    this.pairs = pairs;
    this.nodeModules = path.join(realBase, 'node_modules') + path.sep;
  }

  private async sourceMap(file: string): Promise<SourceMap | null> {
    if (!this.maps.has(file)) {
      let map: SourceMap | null;
      try {
        map = new SourceMap(JSON.parse(await readFile(`${file}.map`, 'utf8')) as SourceMapPayload);
      } catch {
        map = null;
      }
      this.maps.set(file, map);
    }
    return this.maps.get(file) ?? null;
  }

  /** Исходный файл пакета: байты, строки и находки повторного разбора. */
  private async source(file: string) {
    let source = this.sources.get(file);
    if (!source) {
      const bytes = await readFile(file);
      const text = bytes.toString('utf8');
      let findings: LocatedFinding[] | null;
      try {
        findings = scanSourceLocated(text, file);
      } catch {
        findings = null;
      }
      source = { bytes, lines: text.split('\n').map((l) => l.replace(/\r$/, '')), findings };
      this.sources.set(file, source);
    }
    return source;
  }

  async locate(finding: FileFinding): Promise<Located> {
    const absolute = path.resolve(this.base, finding.file);
    const pair = this.pairs.find((p) => absolute.startsWith(p.dist + path.sep));
    if (!pair || !absolute.endsWith('.js')) return { ours: finding.file };
    const mapped = path.join(pair.maps, path.relative(pair.dist, absolute));
    const map = await this.sourceMap(mapped);
    if (!map) return { problem: 'источник не найден по карте исходников (нет карты)' };
    const entry = map.findEntry(finding.line - 1, finding.column - 1) as Partial<{
      generatedLine: number;
      generatedColumn: number;
      originalSource: string;
      originalLine: number;
      originalColumn: number;
    }>;
    if (
      entry.generatedLine !== finding.line - 1 ||
      entry.generatedColumn !== finding.column - 1 ||
      typeof entry.originalSource !== 'string' ||
      typeof entry.originalLine !== 'number' ||
      typeof entry.originalColumn !== 'number'
    ) {
      return { problem: 'источник не найден по карте исходников (нет точного соответствия позиции)' };
    }
    // Источник в карте — путь относительно файла карты (или file:// адрес).
    const source = entry.originalSource.startsWith('file://')
      ? new URL(entry.originalSource).pathname
      : path.resolve(path.dirname(mapped), entry.originalSource);
    let real: string;
    try {
      real = await realpath(source);
    } catch {
      return { problem: `источник не найден по карте исходников (${entry.originalSource})` };
    }
    // Настоящий путь — в node_modules проекта; ссылка из node_modules в наш код — наш код.
    if (!real.startsWith(this.nodeModules)) {
      return { ours: path.relative(this.realBase, real).split(path.sep).join('/') };
    }
    const relative = path.relative(this.realBase, real).split(path.sep);
    const last = relative.lastIndexOf('node_modules');
    const nameLength = relative[last + 1]?.startsWith('@') ? 2 : 1;
    const packageParts = relative.slice(0, last + 1 + nameLength);
    const name = relative.slice(last + 1, last + 1 + nameLength).join('/');
    const lockKey = packageParts.join('/');
    const fileInPackage = relative.slice(last + 1 + nameLength).join('/');
    // Только пакет верхнего уровня: вложенная копия (node_modules/a/node_modules/b)
    // могла бы выдать себя за разрешённый пакет той же версии.
    if (last !== 0) {
      return { problem: `вложенная копия пакета (${lockKey}): исключения действуют только для node_modules/${name}` };
    }
    const lockEntry = this.lock.packages?.[lockKey];
    const version = lockEntry?.version;
    if (lockEntry === undefined || version === undefined) return { problem: `пакета ${lockKey} нет в package-lock.json` };
    if (typeof lockEntry.integrity !== 'string' || !lockEntry.integrity.startsWith('sha512-')) {
      return { problem: `у ${lockKey} в package-lock.json нет integrity (sha512): исключения не действуют` };
    }
    if (lockEntry.inBundle !== undefined || lockEntry.link !== undefined) {
      return { problem: `${lockKey} в package-lock.json — inBundle или link: исключения не действуют` };
    }
    let installed: unknown;
    try {
      installed = (JSON.parse(await readFile(path.join(this.realBase, ...packageParts, 'package.json'), 'utf8')) as {
        version?: unknown;
      }).version;
    } catch {
      installed = undefined;
    }
    if (installed !== version) {
      return {
        problem: `установленный ${name}@${String(installed)} не совпадает с package-lock.json (${version}) — выполните npm ci`,
      };
    }
    const line = entry.originalLine + 1;
    const column = entry.originalColumn + 1;
    const original = await this.source(real);
    // В исходнике на этом месте должна быть та же находка: карта не может
    // приписать находку чужой строке.
    const same = original.findings?.some(
      (f) => f.line === line && f.column === column && f.what === finding.what && f.severity === finding.severity,
    );
    if (!same) {
      return {
        problem: `по карте — ${name}@${version} ${fileInPackage}:${line}:${column}, но в исходнике на этом месте нет такой находки`,
      };
    }
    const text = original.lines[line - 1] ?? '';
    return {
      origin: {
        package: name,
        version,
        file: fileInPackage,
        fileSha256: fileSha256(original.bytes),
        line,
        lineSha256: lineSha256(text),
      },
    };
  }
}

const SEVERITY = { network: 'сеть', suspicious: 'подозрительно' } as const;

function sameRecord(e: Exception, o: Origin, kind: string): boolean {
  return (
    e.package === o.package &&
    e.version === o.version &&
    e.file === o.file &&
    e.fileSha256 === o.fileSha256 &&
    e.line === o.line &&
    e.lineSha256 === o.lineSha256 &&
    e.kind === kind
  );
}

function describeRecord(e: Exception): string {
  return `${e.package}@${e.version} ${e.file}:${e.line} [${e.kind}]`;
}

/** Проверка: находки в targets, сопоставление со списком исключений, проверка копий сборки. */
export async function checkNetwork(options: {
  targets: string[];
  base: string;
  maps: MapPair[];
  allowlist: Exception[];
}): Promise<CheckResult> {
  const { base, allowlist } = options;
  const pairs = options.maps.map((p) => ({ dist: path.resolve(p.dist), maps: path.resolve(p.maps) }));
  const problems: string[] = [];
  for (const pair of pairs) problems.push(...(await compareBuilds(pair, base)));

  const findings = await scanPaths(options.targets, base);
  const realBase = await realpath(base);
  let lock: Lock;
  try {
    lock = JSON.parse(await readFile(path.join(base, 'package-lock.json'), 'utf8')) as Lock;
  } catch {
    lock = {};
  }
  const finder = new OriginFinder(base, realBase, lock, pairs);
  const used = new Map<Exception, number>();
  const allowed: CheckResult['allowed'] = [];

  for (const finding of findings) {
    const head = `${finding.file}:${finding.line}:${finding.column}  [${SEVERITY[finding.severity]}] ${finding.what}`;
    const located = await finder.locate(finding);
    if ('ours' in located) {
      problems.push(`${head} — наш код (${located.ours}): исключения на него не действуют, уберите`);
      continue;
    }
    if ('problem' in located) {
      problems.push(`${head} — ${located.problem}`);
      continue;
    }
    const o = located.origin;
    const from = `из ${o.package}@${o.version} ${o.file}:${o.line}`;
    if (finding.severity === 'network') {
      problems.push(`${head} — ${from} — сетевой вызов: исключения для него не допускаются`);
      continue;
    }
    if (!ALLOWED_KINDS.has(finding.what)) {
      problems.push(`${head} — ${from} — этот вид находки исключением не разрешается: уберите или замените зависимость`);
      continue;
    }
    const exception = allowlist.find((e) => sameRecord(e, o, finding.what));
    if (exception) {
      used.set(exception, (used.get(exception) ?? 0) + 1);
      allowed.push({ finding, origin: o, exception });
      continue;
    }
    // Та же строка с другим текстом или версией, либо тот же текст в другой версии пакета.
    const stale = allowlist.filter(
      (e) =>
        e.package === o.package &&
        e.file === o.file &&
        e.kind === finding.what &&
        (e.line === o.line || (e.lineSha256 === o.lineSha256 && e.version !== o.version)),
    );
    if (stale.length > 0) {
      const was = stale
        .map((e) => `версия ${e.version}, строка ${e.line}, хэш строки ${e.lineSha256.slice(0, 12)}…, хэш файла ${e.fileSha256.slice(0, 12)}…`)
        .join('; ');
      problems.push(
        `${head} — ${from} (хэш строки ${o.lineSha256.slice(0, 12)}…, хэш файла ${o.fileSha256.slice(0, 12)}…) — исключение недействительно (в списке: ${was}): изменилась версия пакета, текст строки или файл, разберите заново`,
      );
    } else {
      problems.push(`${head} — ${from} — исключения нет: разберите находку и добавьте запись в tools/check-no-network/allowlist.json`);
    }
  }

  for (const e of allowlist) {
    const n = used.get(e) ?? 0;
    if (n === 0) {
      problems.push(`Неиспользованное исключение: ${describeRecord(e)} — такой находки в сборке нет; удалите запись или разберите заново`);
    } else if (n !== e.count) {
      problems.push(`Исключение ${describeRecord(e)} покрывает находок: ${n}, а разобрано (count): ${e.count} — разберите заново`);
    }
  }
  return { problems, allowed };
}
