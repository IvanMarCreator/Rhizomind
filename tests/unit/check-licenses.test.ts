import { describe, expect, it } from 'vitest';
import { findLicenseProblems, isAllowedLicense } from '../../scripts/check-licenses.mjs';

describe('isAllowedLicense', () => {
  it.each(['MIT', 'ISC', 'BSD-2-Clause', 'BSD-3-Clause', 'Apache-2.0', '0BSD', 'MPL-2.0', 'Unlicense', 'CC0-1.0',
    'BlueOak-1.0.0', 'Python-2.0', 'GPL-3.0-only', 'GPL-3.0-or-later', 'LGPL-2.1-or-later', 'LGPL-3.0-only'])(
    'разрешает %s',
    (license) => {
      expect(isAllowedLicense(license)).toBe(true);
    },
  );

  it.each(['GPL-2.0-only', 'AGPL-3.0-only', 'CC-BY-NC-4.0', 'SSPL-1.0', 'UNLICENSED', 'WTFPL', '', undefined])(
    'запрещает %s',
    (license) => {
      expect(isAllowedLicense(license)).toBe(false);
    },
  );

  it('выражение OR разрешено, если подходит хотя бы одна лицензия', () => {
    expect(isAllowedLicense('(WTFPL OR MIT)')).toBe(true);
    expect(isAllowedLicense('WTFPL OR CC-BY-NC-4.0')).toBe(false);
  });

  it('выражение AND разрешено, только если подходят все', () => {
    expect(isAllowedLicense('(MIT AND BSD-3-Clause)')).toBe(true);
    expect(isAllowedLicense('MIT AND CC-BY-NC-4.0')).toBe(false);
  });
});

describe('findLicenseProblems', () => {
  const lock = {
    lockfileVersion: 3,
    packages: {
      '': { name: 'rhizomind', license: 'GPL-3.0-only' },
      'node_modules/svelte': { version: '5.0.0', license: 'MIT' },
      'node_modules/bad-runtime': { version: '1.0.0', license: 'CC-BY-NC-4.0' },
      'node_modules/no-license': { version: '2.0.0' },
      'node_modules/bad-dev-tool': { version: '1.0.0', license: 'CC-BY-NC-4.0', dev: true },
      'node_modules/a/node_modules/nested': { version: '3.0.0', license: 'SSPL-1.0' },
    },
  };

  it('проверяет только production-зависимости, включая вложенные', () => {
    expect(findLicenseProblems(lock)).toEqual([
      { name: 'bad-runtime', version: '1.0.0', license: 'CC-BY-NC-4.0' },
      { name: 'no-license', version: '2.0.0', license: '(нет)' },
      { name: 'nested', version: '3.0.0', license: 'SSPL-1.0' },
    ]);
  });

  it('пусто, если все лицензии совместимы', () => {
    expect(findLicenseProblems({ packages: { 'node_modules/@scope/ok': { version: '1.0.0', license: 'ISC' } } })).toEqual([]);
  });
});
