// Снимок реестра HKCU для теста следов (tests/e2e/traces.test.ts, Windows).
//
// Экспорт `reg export HKCU` до запуска и после выхода приложения, сравнение по
// ключам и значениям. Весь HKCU на машине CI всё время меняют фоновые задачи
// Windows (Firefox, IdentityCRL, регистрация пакетов при первом входе…),
// поэтому нарушением считается не любое изменение, а:
//  - любое изменение в местах, где оболочка хранит пути: ComDlg32, RecentDocs,
//    TypedPaths, CIDOpen, ShellBags (BagMRU, Bags) — тест окна не открывает;
//  - любое изменение, в ключе, имени или данных которого (включая строки
//    UTF-16 внутри двоичных значений) есть метка приложения — `rhizomind`,
//    имя временной папки движка, путь хранилища.
// Учтённые записи (docs/SECURITY.md, «Записи вне хранилища») — не нарушение:
// служебные ключи движка WebView2 (решение 61) и записи оболочки Windows о
// запуске программы (MuiCache, UserAssist). Остальное — фон ОС: печатается
// в журнал, тест не валит.

export type RegExport = Map<string, Map<string, string>>;

export interface RegChange {
  kind: 'added' | 'changed' | 'removed';
  key: string;
  /** Имя значения как в экспорте (`"имя"` или `@`); нет — изменение самого ключа. */
  name?: string;
  data?: string;
}

/** Разбор текста `reg export` (уже декодированного из UTF-16). */
export function parseRegExport(text: string): RegExport {
  const result: RegExport = new Map();
  const joined = text.replace(/\\\r?\n\s*/g, '');
  let current: Map<string, string> | undefined;
  for (const raw of joined.split(/\r?\n/)) {
    const line = raw.trimEnd();
    if (line.startsWith('[') && line.endsWith(']')) {
      current = new Map();
      result.set(line.slice(1, -1), current);
      continue;
    }
    if (!current) continue;
    const m = /^("(?:[^"\\]|\\.)*"|@)=(.*)$/.exec(line);
    if (m?.[1] !== undefined && m[2] !== undefined) current.set(m[1], m[2]);
  }
  return result;
}

export function diffRegistry(before: RegExport, after: RegExport): RegChange[] {
  const changes: RegChange[] = [];
  for (const [key, values] of after) {
    const old = before.get(key);
    if (!old) {
      changes.push({ kind: 'added', key });
      for (const [name, data] of values) changes.push({ kind: 'added', key, name, data });
      continue;
    }
    for (const [name, data] of values) {
      const prev = old.get(name);
      if (prev === undefined) changes.push({ kind: 'added', key, name, data });
      else if (prev !== data) changes.push({ kind: 'changed', key, name, data });
    }
    for (const [name, data] of old) {
      if (!values.has(name)) changes.push({ kind: 'removed', key, name, data });
    }
  }
  for (const key of before.keys()) {
    if (!after.has(key)) changes.push({ kind: 'removed', key });
  }
  return changes;
}

/** Данные значения для поиска меток: строки UTF-16 и ASCII из hex:… как текст. */
export function searchableText(data: string): string {
  const m = /^hex(?:\([0-9a-f]+\))?:(.*)$/i.exec(data);
  if (m?.[1] === undefined) return data.replace(/\\\\/g, '\\');
  const bytes = Buffer.from(
    m[1]
      .split(',')
      .filter(Boolean)
      .map((h) => parseInt(h, 16)),
  );
  const utf16 = bytes.subarray(0, bytes.length - (bytes.length % 2)).toString('utf16le');
  return `${utf16}\n${bytes.toString('latin1')}`;
}

/** Места, где оболочка Windows хранит пути к папкам (окно выбора папки, недавние). */
const PATH_HISTORY_KEYS = [
  /\\Explorer\\ComDlg32(\\|$)/i,
  /\\Explorer\\RecentDocs(\\|$)/i,
  /\\Explorer\\TypedPaths(\\|$)/i,
  /\\Explorer\\CIDOpen(\\|$)/i,
  /\\Shell\\BagMRU(\\|$)/i,
  /\\Shell\\Bags(\\|$)/i,
];

/** Учтённые записи (docs/SECURITY.md, «Записи вне хранилища»). */
const KNOWN_KEYS: { pattern: RegExp; why: string }[] = [
  { pattern: /^HKEY_CURRENT_USER\\Software\\Microsoft\\EdgeWebView(\\|$)/i, why: 'движок WebView2, решение 61' },
  {
    pattern: /^HKEY_CURRENT_USER\\Software\\Microsoft\\EdgeUpdate\\ClientState\\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5\}(\\|$)/i,
    why: 'движок WebView2, решение 61',
  },
  { pattern: /\\Shell\\MuiCache(\\|$)/i, why: 'оболочка Windows: запуск программы' },
  { pattern: /\\Explorer\\UserAssist\\/i, why: 'оболочка Windows: запуск программы' },
];

export interface RegistryCheck {
  violations: RegChange[];
  known: (RegChange & { why: string })[];
  background: RegChange[];
}

/**
 * Раскладывает изменения HKCU на нарушения, учтённые записи и фон ОС.
 * `markers` — подстроки, по которым узнаётся приложение (без учёта регистра).
 */
export function checkRegistryChanges(changes: RegChange[], markers: string[]): RegistryCheck {
  const lower = markers.filter(Boolean).map((m) => m.toLowerCase());
  const result: RegistryCheck = { violations: [], known: [], background: [] };
  for (const change of changes) {
    const known = KNOWN_KEYS.find((k) => k.pattern.test(change.key));
    if (known) {
      result.known.push({ ...change, why: known.why });
      continue;
    }
    const text = `${change.key}\n${change.name ?? ''}\n${change.data ? searchableText(change.data) : ''}`.toLowerCase();
    const mentionsApp = lower.some((m) => text.includes(m));
    if (mentionsApp || PATH_HISTORY_KEYS.some((p) => p.test(change.key))) result.violations.push(change);
    else result.background.push(change);
  }
  return result;
}

/** Короткая сводка фона: число изменений по первым уровням пути ключа. */
export function summarize(changes: RegChange[], depth = 6, limit = 15): string {
  const counts = new Map<string, number>();
  for (const c of changes) {
    const prefix = c.key.split('\\').slice(0, depth).join('\\');
    counts.set(prefix, (counts.get(prefix) ?? 0) + 1);
  }
  return [...counts]
    .sort((a, b) => b[1] - a[1])
    .slice(0, limit)
    .map(([k, n]) => `${n} ${k}`)
    .join('; ');
}
