import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import {
  DESKTOP_TARGETS,
  cargoTreeArgs,
  compareWithBaseline,
  crateNames,
  deniedCrates,
  findNetworkCrates,
} from './tree.ts';

// Вывод `cargo tree -e normal --prefix none --format "{p}|{f}"`.
const tree = (...lines: string[]) => lines.join('\n') + '\n';

describe('findNetworkCrates', () => {
  it('чистое дерево — пусто', () => {
    const out = tree(
      'rhizomind v0.1.0 (/w/src-tauri)|',
      'tauri v2.12.5|common-controls-v6,compression,default,wry',
      'tokio v1.48.0|rt,sync,time',
      'mio v1.1.0|os-ext,os-poll',
      'serde v1.0.229 (*)|default,derive,std',
      'h2o-like v1.0.0|',
      'thttp v0.1.0|',
    );
    expect(findNetworkCrates(out)).toEqual([]);
  });

  it.each([
    'hyper v1.7.0|client,http1',
    'hyper-util v0.1.17|',
    'reqwest v0.12.24|default',
    'ureq v3.1.2|',
    'h2 v0.4.12|',
    'h3 v0.0.8|',
    'rustls v0.23.33|ring',
    'tokio-rustls v0.26.4|',
    'rustls-pki-types v1.12.0|',
    'native-tls v0.2.14|',
    'openssl v0.10.74|',
    'openssl-sys v0.9.110|',
    'socket2 v0.6.1|',
    'tungstenite v0.27.0|',
    'tokio-tungstenite v0.27.0|',
    'quinn v0.11.9|',
    'curl v0.4.49|',
    'curl-sys v0.4.83+curl-8.15.0|',
  ])('находит %s', (line) => {
    expect(findNetworkCrates(tree('rhizomind v0.1.0 (/w)|', line))).toEqual([line.split('|')[0]]);
  });

  it('tokio или mio с функцией net — находка', () => {
    expect(findNetworkCrates(tree('tokio v1.48.0|net,rt', 'mio v1.1.0|net,os-poll'))).toEqual([
      'tokio v1.48.0 (функция net)',
      'mio v1.1.0 (функция net)',
    ]);
  });

  it('повтор одного крейта показывается один раз', () => {
    expect(findNetworkCrates(tree('hyper v1.7.0|', 'hyper v1.7.0 (*)|'))).toEqual(['hyper v1.7.0']);
  });

  it('пустой вывод — ошибка: дерево не построено, а не «сети нет»', () => {
    expect(() => findNetworkCrates('')).toThrow();
  });
});

describe('cargoTreeArgs', () => {
  it('нормальные зависимости для одной цели без процедурных макросов, без отступов, с функциями', () => {
    // no-proc-macro: макросы собираются для машины сборки, их зависимости
    // зависят от ОС раннера (на Windows — winapi-util и др. у любой цели).
    expect(cargoTreeArgs('x86_64-pc-windows-msvc', 'src-tauri/Cargo.toml')).toEqual([
      'tree',
      '--manifest-path',
      'src-tauri/Cargo.toml',
      '--locked',
      '-e',
      'normal,no-proc-macro',
      '--target',
      'x86_64-pc-windows-msvc',
      '--prefix',
      'none',
      '--format',
      '{p}|{f}',
    ]);
  });

  it('проверяются все десктопные цели', () => {
    expect(DESKTOP_TARGETS).toEqual([
      'x86_64-pc-windows-msvc',
      'aarch64-pc-windows-msvc',
      'x86_64-apple-darwin',
      'aarch64-apple-darwin',
      'x86_64-unknown-linux-gnu',
    ]);
  });
});

describe('запреты из deny.toml (ревью 0.2, п. 11)', () => {
  const deny = readFileSync(path.join(import.meta.dirname, '..', '..', 'src-tauri', 'deny.toml'), 'utf8');

  it('берёт имена крейтов из [bans] deny', () => {
    expect(deniedCrates('[bans]\ndeny = [\n  { crate = "reqwest" }, { crate = "tiny_http" },\n]\n')).toEqual(
      new Set(['reqwest', 'tiny_http']),
    );
  });

  it.each([
    'tauri-plugin-http',
    'tauri-plugin-updater',
    'tauri-plugin-shell',
    'tauri-plugin-localhost',
    'tauri-plugin-websocket',
    'tauri-plugin-upload',
    'isahc',
    'attohttpc',
    'surf',
    'tiny_http',
    'hyperlocal',
    'axum',
    'async-net',
    'hickory-resolver',
    'ws',
    'websocket',
    'lettre',
    'russh',
    'libssh2-sys',
  ])('deny.toml запрещает %s, и проверка дерева его находит', (name) => {
    const denied = deniedCrates(deny);
    expect(denied.has(name)).toBe(true);
    expect(findNetworkCrates(tree('rhizomind v0.1.0 (/w)|', `${name} v1.0.0|`), denied)).toEqual([`${name} v1.0.0`]);
  });

  it('soup3 (Linux: привязки WebKitGTK к libsoup) — явное исключение, другие *soup* — находка', () => {
    const denied = deniedCrates(deny);
    expect(findNetworkCrates(tree('soup3 v0.5.0|', 'soup3-sys v0.5.0|'), denied)).toEqual([]);
    expect(findNetworkCrates(tree('soup2 v0.2.1|'), denied)).toEqual(['soup2 v0.2.1']);
  });
});

describe('список крейтов по целям (ревью 0.2, п. 11)', () => {
  it('имена без версий и повторов, по алфавиту', () => {
    expect(crateNames(tree('tauri v2.12.5|x', 'serde v1.0.1 (*)|', 'serde v1.0.1|', 'base64 v0.22.1|', 'base64 v0.23.1|'))).toEqual([
      'base64',
      'serde',
      'tauri',
    ]);
  });

  it('новый крейт и пропавший крейт — расхождение', () => {
    expect(compareWithBaseline(['a', 'b', 'new'], ['a', 'b', 'gone'])).toEqual({ added: ['new'], removed: ['gone'] });
    expect(compareWithBaseline(['a'], ['a'])).toEqual({ added: [], removed: [] });
  });

  it('в репозитории есть список для каждой десктопной цели', () => {
    const baseline = JSON.parse(readFileSync(path.join(import.meta.dirname, 'crates.json'), 'utf8')) as Record<
      string,
      string[]
    >;
    expect(Object.keys(baseline).sort()).toEqual([...DESKTOP_TARGETS].sort());
    for (const target of DESKTOP_TARGETS) expect(baseline[target]?.length).toBeGreaterThan(50);
  });
});
