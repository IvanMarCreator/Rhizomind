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

export function cargoTreeArgs(target: string, manifestPath: string): string[] {
  return [
    'tree',
    '--manifest-path',
    manifestPath,
    '--locked',
    '-e',
    'normal',
    '--target',
    target,
    '--prefix',
    'none',
    '--format',
    '{p}|{f}',
  ];
}

/** Сетевые крейты в выводе cargo tree (формат `{p}|{f}`), без повторов, в порядке вывода. */
export function findNetworkCrates(output: string): string[] {
  const lines = output.split(/\r?\n/).filter((l) => l.trim() !== '');
  if (lines.length === 0) throw new Error('cargo tree ничего не вывел — дерево зависимостей не построено');

  const hits = new Set<string>();
  for (const line of lines) {
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
