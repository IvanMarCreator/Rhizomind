import { describe, expect, it } from 'vitest';
import { findLicenseProblems, isAllowedLicense } from './licenses.ts';

describe('isAllowedLicense: выражения SPDX', () => {
  it.each(['MIT', 'ISC', 'Apache-2.0', 'BSD-3-Clause', 'GPL-3.0-only', 'GPL-3.0-or-later', '0BSD'])(
    'разрешает %s',
    (expr) => expect(isAllowedLicense(expr)).toBe(true),
  );

  it.each(['GPL-2.0-only', 'SSPL-1.0', 'BUSL-1.1', 'Commercial', 'UNLICENSED', 'SEE LICENSE IN LICENSE.txt'])(
    'отклоняет %s',
    (expr) => expect(isAllowedLicense(expr)).toBe(false),
  );

  it.each([
    ['(MIT OR Apache-2.0)', true],
    ['Apache-2.0 OR MIT', true],
    ['MIT OR GPL-2.0-only', true],
    ['(MIT AND ISC)', true],
    ['MIT AND GPL-2.0-only', false],
    ['BUSL-1.1 AND (MIT OR ISC)', false],
    ['(MIT OR ISC) AND BUSL-1.1', false],
    ['BUSL-1.1 OR (MIT AND ISC)', true],
    ['((MIT))', true],
    ['MIT AND ISC OR BUSL-1.1', true],
    ['BUSL-1.1 AND MIT OR SSPL-1.0', false],
    ['Apache-2.0 WITH LLVM-exception', true],
    ['GPL-2.0-only WITH Classpath-exception-2.0', false],
    ['mit or isc', false],
  ])('%s → %s', (expr, allowed) => {
    expect(isAllowedLicense(expr)).toBe(allowed);
  });

  it.each(['', '   ', '(MIT', 'MIT)', 'MIT OR', 'AND MIT', 'MIT ISC', '()', 'MIT WITH'])(
    'неверное выражение «%s» — отказ',
    (expr) => expect(isAllowedLicense(expr)).toBe(false),
  );

  it('не строка — отказ', () => {
    expect(isAllowedLicense(undefined)).toBe(false);
    expect(isAllowedLicense({ type: 'MIT' })).toBe(false);
    expect(isAllowedLicense(['MIT'])).toBe(false);
  });
});

describe('findLicenseProblems: package-lock.json', () => {
  const lock = {
    lockfileVersion: 3,
    packages: {
      '': { name: 'rhizomind', license: 'GPL-3.0-only' },
      'node_modules/good': { version: '1.0.0', license: 'MIT' },
      'node_modules/dual': { version: '2.0.0', license: '(MIT OR Apache-2.0)' },
      'node_modules/bad': { version: '1.2.3', license: 'BUSL-1.1 AND (MIT OR ISC)' },
      'node_modules/none': { version: '0.1.0' },
      'node_modules/good/node_modules/@scope/nested': { version: '3.0.0', license: 'SSPL-1.0' },
      'node_modules/devonly': { version: '1.0.0', license: 'BUSL-1.1', dev: true },
      'node_modules/devopt': { version: '1.0.0', license: 'BUSL-1.1', devOptional: true },
      'node_modules/platform-binary': { version: '1.0.0', license: 'BUSL-1.1', optional: true },
      'node_modules/linked': { link: true, resolved: 'tools/x' },
    },
  };

  it('сообщает о production-зависимостях с несовместимой или неуказанной лицензией', () => {
    expect(findLicenseProblems(lock)).toEqual([
      { name: 'bad', version: '1.2.3', license: 'BUSL-1.1 AND (MIT OR ISC)' },
      { name: 'none', version: '0.1.0', license: '(нет)' },
      { name: '@scope/nested', version: '3.0.0', license: 'SSPL-1.0' },
      { name: 'platform-binary', version: '1.0.0', license: 'BUSL-1.1' },
    ]);
  });

  it('lockfile без packages (старый формат) — ошибка, а не «всё хорошо»', () => {
    expect(() => findLicenseProblems({ lockfileVersion: 1, dependencies: {} })).toThrow(/lockfileVersion/);
  });
});
