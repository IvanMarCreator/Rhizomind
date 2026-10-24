// Лицензии production-зависимостей интерфейса (то, что попадает в сборку):
// каждая совместима с GPL-3.0. Выражения SPDX разбираются по грамматике со
// скобками и приоритетом WITH > AND > OR: строковое сравнение пропускает
// `BUSL-1.1 AND (MIT OR ISC)` или ошибочно отклоняет `(MIT OR Apache-2.0)` (ADR 0009).

// Лицензии, совместимые с GPL-3.0. Добавление — только с записью в docs/dependencies.md.
const ALLOWED_IDS = new Set([
  'MIT',
  'ISC',
  'BSD-2-Clause',
  'BSD-3-Clause',
  'Apache-2.0',
  '0BSD',
  'Zlib',
  'CC0-1.0',
  'Unlicense',
  'MPL-2.0',
  'BlueOak-1.0.0',
  'GPL-3.0-only',
  'GPL-3.0-or-later',
  'LGPL-3.0-only',
  'LGPL-3.0-or-later',
]);

// Лицензия с исключением разрешена, только если разрешена именно эта пара.
const ALLOWED_WITH = new Set(['Apache-2.0 WITH LLVM-exception']);

type Expr =
  | { kind: 'id'; id: string; exception?: string }
  | { kind: 'and' | 'or'; left: Expr; right: Expr };

const TOKEN = /\s*(\(|\)|[A-Za-z0-9.+:-]+)/y;

function tokenize(text: string): string[] {
  const tokens: string[] = [];
  TOKEN.lastIndex = 0;
  while (TOKEN.lastIndex < text.length) {
    const start = TOKEN.lastIndex;
    const match = TOKEN.exec(text);
    if (!match) {
      if (text.slice(start).trim() === '') break;
      throw new Error(`недопустимый символ в «${text}»`);
    }
    tokens.push(match[1] as string);
  }
  return tokens;
}

const KEYWORDS = new Set(['AND', 'OR', 'WITH', '(', ')']);

/** Разбор выражения SPDX в дерево; ошибка при неверном выражении. */
export function parseSpdx(text: string): Expr {
  const tokens = tokenize(text);
  let pos = 0;
  const peek = () => tokens[pos];
  const take = (expected?: string) => {
    const token = tokens[pos];
    if (token === undefined || (expected !== undefined && token !== expected)) {
      throw new Error(`ожидалось ${expected ?? 'продолжение'} в «${text}»`);
    }
    pos += 1;
    return token;
  };

  const atom = (): Expr => {
    if (peek() === '(') {
      take('(');
      const inner = or();
      take(')');
      return inner;
    }
    const id = take();
    if (KEYWORDS.has(id)) throw new Error(`ожидалась лицензия, а не ${id} в «${text}»`);
    if (peek() === 'WITH') {
      take('WITH');
      const exception = take();
      if (KEYWORDS.has(exception)) throw new Error(`ожидалось исключение после WITH в «${text}»`);
      return { kind: 'id', id, exception };
    }
    return { kind: 'id', id };
  };
  const and = (): Expr => {
    let left = atom();
    while (peek() === 'AND') {
      take('AND');
      left = { kind: 'and', left, right: atom() };
    }
    return left;
  };
  const or = (): Expr => {
    let left = and();
    while (peek() === 'OR') {
      take('OR');
      left = { kind: 'or', left, right: and() };
    }
    return left;
  };

  const result = or();
  if (pos !== tokens.length) throw new Error(`лишнее «${tokens[pos] ?? ''}» в «${text}»`);
  return result;
}

function allowed(expr: Expr): boolean {
  switch (expr.kind) {
    case 'id':
      return expr.exception === undefined
        ? ALLOWED_IDS.has(expr.id)
        : ALLOWED_WITH.has(`${expr.id} WITH ${expr.exception}`);
    case 'and':
      return allowed(expr.left) && allowed(expr.right);
    case 'or':
      return allowed(expr.left) || allowed(expr.right);
  }
}

/** Лицензия (поле license из package-lock.json) совместима с GPL-3.0. Неверное выражение — нет. */
export function isAllowedLicense(license: unknown): boolean {
  if (typeof license !== 'string' || license.trim() === '') return false;
  try {
    return allowed(parseSpdx(license));
  } catch {
    return false;
  }
}

interface LockPackage {
  version?: string;
  license?: unknown;
  dev?: boolean;
  devOptional?: boolean;
  link?: boolean;
}

export interface LicenseProblem {
  name: string;
  version: string;
  license: string;
}

/** Пакеты, которые ставит `npm ci --omit=dev`: всё, кроме dev, и без ссылок на локальные папки. */
export function productionPackages(lock: unknown): [string, LockPackage][] {
  const packages = (lock as { packages?: Record<string, LockPackage> }).packages;
  if (!packages) {
    throw new Error('package-lock.json без раздела packages: нужен lockfileVersion 2 или 3');
  }
  return Object.entries(packages).filter(([key, info]) => key !== '' && !info.dev && !info.devOptional && !info.link);
}

/** Нарушения в package-lock.json в порядке файла. */
export function findLicenseProblems(lock: unknown): LicenseProblem[] {
  const problems: LicenseProblem[] = [];
  for (const [key, info] of productionPackages(lock)) {
    if (isAllowedLicense(info.license)) continue;
    const name = key.slice(key.lastIndexOf('node_modules/') + 'node_modules/'.length);
    const license = typeof info.license === 'string' ? info.license : info.license ? JSON.stringify(info.license) : '(нет)';
    problems.push({ name, version: info.version ?? '?', license });
  }
  return problems;
}
