// Независимая от cargo-deny проверка: в нормальных зависимостях ядра нет
// сетевых крейтов ни для одной десктопной цели (ADR 0009). Список имён — из
// постановки задачи 0.2 и исследования крейтов; совпадение ищется по части
// имени между дефисами, поэтому ловятся и обёртки (`tokio-rustls`, `curl-sys`).

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
  'quinn',
  'curl',
]);

// Крейты, которые допустимы только без сетевой функции.
const NET_FEATURE_CRATES = new Set(['tokio', 'mio']);

// Крейты только для тестов (dev-зависимости ядра): в программу и в сборку
// попадать не должны (решение оркестратора по ревью 1.1).
const TEST_ONLY_CRATES = new Set(['gen-vault', 'proptest']);

export function cargoTreeArgs(
  target: string,
  manifestPath: string,
  edges: 'normal' | 'normal,build' = 'normal',
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

function treeLines(output: string): string[] {
  const lines = output.split(/\r?\n/).filter((l) => l.trim() !== '');
  if (lines.length === 0) throw new Error('cargo tree ничего не вывел — дерево зависимостей не построено');
  return lines;
}

/** Крейты только для тестов в выводе cargo tree (формат `{p}|{f}`), без повторов. */
export function findTestOnlyCrates(output: string): string[] {
  const hits = new Set<string>();
  for (const line of treeLines(output)) {
    const [name = '', version = ''] = (line.split('|')[0] ?? '').trim().split(/\s+/);
    if (TEST_ONLY_CRATES.has(name)) hits.add(`${name} ${version}`);
  }
  return [...hits];
}

/** Сетевые крейты в выводе cargo tree (формат `{p}|{f}`), без повторов, в порядке вывода. */
export function findNetworkCrates(output: string): string[] {
  const hits = new Set<string>();
  for (const line of treeLines(output)) {
    const [pkg = '', features = ''] = line.split('|');
    const [name = '', version = ''] = pkg.trim().split(/\s+/);
    const parts = name.split(/[-_]/);
    if (parts.some((p) => NETWORK_PARTS.has(p)) || name.includes('native-tls')) {
      hits.add(`${name} ${version}`);
    } else if (NET_FEATURE_CRATES.has(name) && features.split(',').includes('net')) {
      hits.add(`${name} ${version} (функция net)`);
    }
  }
  return [...hits];
}
