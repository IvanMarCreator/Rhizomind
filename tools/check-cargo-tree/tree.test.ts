import { describe, expect, it } from 'vitest';
import { DESKTOP_TARGETS, cargoTreeArgs, findNetworkCrates, findTestOnlyCrates } from './tree.ts';

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

describe('findTestOnlyCrates', () => {
  it('без крейтов для тестов — пусто', () => {
    const out = tree('rhizomind v0.1.0 (/w/src-tauri)|', 'tauri-build v2.7.4|', 'gen-vault-like v1.0.0|', 'proptest-derive v0.5.0|');
    expect(findTestOnlyCrates(out)).toEqual([]);
  });

  it('gen-vault и proptest в нормальных или сборочных зависимостях — находка', () => {
    const out = tree(
      'rhizomind v0.1.0 (/w/src-tauri)|',
      'gen-vault v0.1.0 (/w/tools/gen-vault)|',
      'proptest v1.12.0|std',
      'proptest v1.12.0 (*)|std',
    );
    expect(findTestOnlyCrates(out)).toEqual(['gen-vault v0.1.0', 'proptest v1.12.0']);
  });

  it('пустой вывод — ошибка', () => {
    expect(() => findTestOnlyCrates('')).toThrow();
  });
});

describe('cargoTreeArgs', () => {
  it('нормальные и сборочные зависимости — для проверки крейтов тестов', () => {
    expect(cargoTreeArgs('x86_64-apple-darwin', 'src-tauri/Cargo.toml', 'normal,build')).toContain('normal,build');
  });

  it('нормальные зависимости для одной цели, без отступов, с функциями', () => {
    expect(cargoTreeArgs('x86_64-pc-windows-msvc', 'src-tauri/Cargo.toml')).toEqual([
      'tree',
      '--manifest-path',
      'src-tauri/Cargo.toml',
      '--locked',
      '-e',
      'normal',
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
