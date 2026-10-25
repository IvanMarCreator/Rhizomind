// Независимая от cargo-deny проверка: в нормальных зависимостях ядра нет
// сетевых крейтов ни для одной десктопной цели (ADR 0009).
// - Точные имена — из `[bans] deny` в src-tauri/deny.toml (один список на обе проверки).
// - Части имён между дефисами — ловят обёртки и новые крейты (`tokio-rustls`, `curl-sys`).
// - Список всех крейтов по целям хранится в crates.json: новый крейт валит
//   проверку, пока его не посмотрят и не обновят список (`--update`).

export const DESKTOP_TARGETS = [
  'x86_64-pc-windows-msvc',
  'aarch64-pc-windows-msvc',
  'x86_64-apple-darwin',
  'aarch64-apple-darwin',
  'x86_64-unknown-linux-gnu',
] as const;

const NETWORK_PARTS = new Set([
  'hyper',
  'reqwest',
  'ureq',
  'h2',
  'h3',
  'rustls',
  'openssl',
  'socket2',
  'tungstenite',
  'websocket',
  'quinn',
  'curl',
  'hickory',
  'libssh2',
  'soup',
]);

// Совпадения по части имени, которые разрешены осознанно.
const ALLOWED = new Map([
  // Linux: привязки к системной libsoup, через которую WebKitGTK обслуживает
  // свои схемы адресов (tauri://, ipc://). Сетевых запросов Rhizomind через неё
  // не делает; сеть страницы закрыта CSP и запретом переходов.
  ['soup3', 'WebKitGTK (Linux)'],
  ['soup3-sys', 'WebKitGTK (Linux)'],
]);

// Крейты, которые допустимы только без сетевой функции.
const NET_FEATURE_CRATES = new Set(['tokio', 'mio']);

// Крейты только для тестов (dev-зависимости ядра): в программу и в сборку
// попадать не должны (решение оркестратора по ревью 1.1).
const TEST_ONLY_CRATES = new Set(['gen-vault', 'proptest']);

/** Имена крейтов из `[bans] deny = [ { crate = "…" }, … ]`. */
export function deniedCrates(denyToml: string): Set<string> {
  const bans = denyToml.split(/^\[bans\]\s*$/m)[1] ?? '';
  const list = bans.match(/^deny\s*=\s*\[([\s\S]*?)^\]/m)?.[1] ?? '';
  return new Set([...list.matchAll(/crate\s*=\s*"([^"]+)"/g)].map((m) => m[1] ?? ''));
}

// `no-proc-macro`: процедурные макросы и их зависимости собираются для машины
// сборки, а не для цели, — их набор зависел бы от ОС раннера CI (на Windows у
// любой цели появлялись winapi-util, windows-sys). В программу они не попадают;
// сетевые крейты во всём графе, включая макросы, запрещает `cargo deny` (bans).
// `normal,build` — для проверки, что крейты тестов не попадают и в сборку.
export function cargoTreeArgs(
  target: string,
  manifestPath: string,
  edges: 'normal,no-proc-macro' | 'normal,build' = 'normal,no-proc-macro',
): string[] {
  return [
    'tree',
    '--manifest-path',
    manifestPath,
    '--locked',
    '-e',
    edges,
    '--target',
    target,
    '--prefix',
    'none',
    '--format',
    '{p}|{f}',
  ];
}

function parseLines(output: string): { name: string; version: string; features: string[] }[] {
  const lines = output.split(/\r?\n/).filter((l) => l.trim() !== '');
  if (lines.length === 0) throw new Error('cargo tree ничего не вывел — дерево зависимостей не построено');
  return lines.map((line) => {
    const [pkg = '', features = ''] = line.split('|');
    const [name = '', version = ''] = pkg.trim().split(/\s+/);
    return { name, version, features: features.split(',') };
  });
}

/** Крейты только для тестов в выводе cargo tree (формат `{p}|{f}`), без повторов. */
export function findTestOnlyCrates(output: string): string[] {
  const hits = new Set<string>();
  for (const { name, version } of parseLines(output)) {
    if (TEST_ONLY_CRATES.has(name)) hits.add(`${name} ${version}`);
  }
  return [...hits];
}

/** Сетевые крейты в выводе cargo tree (формат `{p}|{f}`), без повторов, в порядке вывода. */
export function findNetworkCrates(output: string, denied: Set<string> = new Set()): string[] {
  const hits = new Set<string>();
  for (const { name, version, features } of parseLines(output)) {
    const parts = name.split(/[-_]/);
    // Часть с номером версии в имени тоже считается: soup2 → soup.
    const byPart = parts.some((p) => NETWORK_PARTS.has(p) || NETWORK_PARTS.has(p.replace(/\d+$/, '')));
    const byName = denied.has(name) || byPart || name.includes('native-tls');
    if (byName && !ALLOWED.has(name)) {
      hits.add(`${name} ${version}`);
    } else if (NET_FEATURE_CRATES.has(name) && features.includes('net')) {
      hits.add(`${name} ${version} (функция net)`);
    }
  }
  return [...hits];
}

/** Имена всех крейтов дерева без версий и повторов, по алфавиту. */
export function crateNames(output: string): string[] {
  return [...new Set(parseLines(output).map((l) => l.name))].sort();
}

/** Чем дерево отличается от списка в репозитории. */
export function compareWithBaseline(actual: string[], baseline: string[]): { added: string[]; removed: string[] } {
  const known = new Set(baseline);
  const present = new Set(actual);
  return {
    added: actual.filter((n) => !known.has(n)),
    removed: baseline.filter((n) => !present.has(n)),
  };
}
