// Снимок реестра HKCU для теста следов (tests/e2e/traces.test.ts, Windows).
//
// Экспорт `reg export HKCU` до запуска и после выхода приложения, сравнение по
// ключам и значениям. Весь HKCU на машине CI всё время меняют фоновые задачи
// Windows (Firefox, IdentityCRL, регистрация пакетов при первом входе…),
// поэтому нарушением считается не любое изменение, а:
//  - любое изменение в местах, где оболочка хранит пути: ComDlg32, RecentDocs,
//    TypedPaths, CIDOpen, ShellBags (BagMRU, Bags) — тест окна не открывает;
//  - изменение вне учтённых ключей, в ключе, имени или данных которого
//    (включая строки UTF-16 внутри двоичных значений, с чётного и нечётного
//    байта) есть метка приложения — `rhizomind`, имя временной папки движка;
//  - где угодно, в том числе в учтённых ключах, — метка данных: случайные
//    имена тестового хранилища, заметки и введённого текста. Их тест ищет
//    и во всём экспорте «после» ([`findMarkers`]), а не только в изменениях.
// Учтённые записи (docs/SECURITY.md, «Записи вне хранилища») без меток данных —
// не нарушение: служебные ключи движка WebView2 (решение 61) и записи оболочки
// Windows о запуске программы (MuiCache, UserAssist). Остальное — фон ОС:
// печатается в журнал, тест не валит.

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

/**
 * Данные значения для поиска меток: для hex:… — строки UTF-16LE с чётного и
 * с нечётного байта (имена внутри PIDL выравнены как придётся) и байты как
 * latin1; для строк — текст без экранирования `\\`.
 */
export function searchableText(data: string): string {
  const m = /^hex(?:\([0-9a-f]+\))?:(.*)$/i.exec(data);
  if (m?.[1] === undefined) return data.replace(/\\\\/g, '\\');
  const bytes = Buffer.from(
    m[1]
      .split(',')
      .filter(Boolean)
      .map((h) => parseInt(h, 16)),
  );
  const utf16 = (from: number) => {
    const part = bytes.subarray(from);
    return part.subarray(0, part.length - (part.length % 2)).toString('utf16le');
  };
  return `${utf16(0)}\n${utf16(1)}\n${bytes.toString('latin1')}`;
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

/** Метки (без учёта регистра): `app` — приложение вообще, `data` — данные хранилища. */
export interface Markers {
  app: string[];
  data: string[];
}

function changeText(change: { key: string; name?: string; data?: string }): string {
  return `${change.key}\n${change.name ?? ''}\n${change.data ? searchableText(change.data) : ''}`.toLowerCase();
}

const lowered = (list: string[]) => list.filter(Boolean).map((m) => m.toLowerCase());

/** Раскладывает изменения HKCU на нарушения, учтённые записи и фон ОС. */
export function checkRegistryChanges(changes: RegChange[], markers: Markers): RegistryCheck {
  const app = lowered(markers.app);
  const data = lowered(markers.data);
  const result: RegistryCheck = { violations: [], known: [], background: [] };
  for (const change of changes) {
    const text = changeText(change);
    if (data.some((m) => text.includes(m))) {
      result.violations.push(change);
      continue;
    }
    const known = KNOWN_KEYS.find((k) => k.pattern.test(change.key));
    if (known) {
      result.known.push({ ...change, why: known.why });
      continue;
    }
    if (app.some((m) => text.includes(m)) || PATH_HISTORY_KEYS.some((p) => p.test(change.key))) result.violations.push(change);
    else result.background.push(change);
  }
  return result;
}

export interface MarkerHit {
  key: string;
  name?: string;
  marker: string;
}

/** Все места экспорта (ключи и значения), где встречается одна из меток. */
export function findMarkers(reg: RegExport, markers: string[]): MarkerHit[] {
  const list = markers.filter(Boolean);
  const hits: MarkerHit[] = [];
  const match = (text: string) => list.find((m) => text.includes(m.toLowerCase()));
  for (const [key, values] of reg) {
    const inKey = match(key.toLowerCase());
    if (inKey) hits.push({ key, marker: inKey });
    for (const [name, data] of values) {
      const found = match(changeText({ key: '', name, data }));
      if (found) hits.push({ key, name, marker: found });
    }
  }
  return hits;
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
