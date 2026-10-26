import { describe, expect, it } from 'vitest';
import { checkRegistryChanges, diffRegistry, parseRegExport, searchableText } from './registry.ts';

const HKCU = 'HKEY_CURRENT_USER';

function exportText(body: string): string {
  return `Windows Registry Editor Version 5.00\r\n\r\n${body}`;
}

/** Строка hex:… с текстом в UTF-16LE (так в реестре лежат PIDL и пути). */
function hexUtf16(text: string): string {
  return 'hex:' + [...Buffer.from(text, 'utf16le')].map((b) => b.toString(16).padStart(2, '0')).join(',');
}

describe('parseRegExport', () => {
  it('читает ключи и значения, склеивает продолженные строки', () => {
    const text = exportText(
      `[${HKCU}\\Software\\A]\r\n"x"=dword:00000001\r\n"y"=hex:01,02,\\\r\n  03\r\n@="def"\r\n\r\n[${HKCU}\\Software\\B]\r\n`,
    );
    const reg = parseRegExport(text);
    expect([...reg.keys()]).toEqual([`${HKCU}\\Software\\A`, `${HKCU}\\Software\\B`]);
    expect(reg.get(`${HKCU}\\Software\\A`)).toEqual(
      new Map([
        ['"x"', 'dword:00000001'],
        ['"y"', 'hex:01,02,03'],
        ['@', '"def"'],
      ]),
    );
    expect(reg.get(`${HKCU}\\Software\\B`)?.size).toBe(0);
  });
});

describe('diffRegistry', () => {
  it('находит новые, изменённые и удалённые ключи и значения', () => {
    const before = parseRegExport(exportText(`[${HKCU}\\S\\K]\r\n"a"="1"\r\n"b"="2"\r\n[${HKCU}\\S\\Old]\r\n`));
    const after = parseRegExport(exportText(`[${HKCU}\\S\\K]\r\n"a"="9"\r\n"c"="3"\r\n[${HKCU}\\S\\New]\r\n"z"="0"\r\n`));
    const changes = diffRegistry(before, after).map((c) => `${c.kind} ${c.key} ${c.name ?? ''}`.trim());
    expect(changes.sort()).toEqual(
      [
        `changed ${HKCU}\\S\\K "a"`,
        `added ${HKCU}\\S\\K "c"`,
        `removed ${HKCU}\\S\\K "b"`,
        `added ${HKCU}\\S\\New`,
        `added ${HKCU}\\S\\New "z"`,
        `removed ${HKCU}\\S\\Old`,
      ].sort(),
    );
  });
});

describe('searchableText', () => {
  it('раскрывает строки UTF-16 внутри двоичного значения', () => {
    expect(searchableText(hexUtf16('C:\\Vaults\\Rhizomind-test'))).toContain('C:\\Vaults\\Rhizomind-test');
  });
});

describe('checkRegistryChanges', () => {
  const markers = ['rhizomind', 'rhizomind-webview-123-abc'];
  const check = (key: string, name: string, data: string) =>
    checkRegistryChanges([{ kind: 'added', key, name, data }], markers);

  it('любая правка ComDlg32, RecentDocs, ShellBags — нарушение', () => {
    expect(check(`${HKCU}\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\ComDlg32\\CIDSizeMRU`, '"0"', 'hex:00').violations).toHaveLength(1);
    expect(check(`${HKCU}\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\RecentDocs`, '"0"', 'hex:00').violations).toHaveLength(1);
    expect(
      check(`${HKCU}\\Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\Shell\\BagMRU\\0`, '"0"', 'hex:00').violations,
    ).toHaveLength(1);
  });

  it('упоминание приложения в чужом ключе — нарушение, в том числе внутри двоичного значения', () => {
    expect(check(`${HKCU}\\Software\\Foo`, '"Path"', '"D:\\\\x\\\\Rhizomind\\\\vault"').violations).toHaveLength(1);
    expect(check(`${HKCU}\\Software\\Foo`, '"Bin"', hexUtf16('rhizomind-webview-123-abc')).violations).toHaveLength(1);
  });

  it('записи движка WebView2 (решение 61) и записи оболочки о запуске программы — учтены, не нарушение', () => {
    const r1 = check(`${HKCU}\\Software\\Microsoft\\EdgeWebView\\BLBeacon`, '"version"', '"155"');
    const r2 = check(`${HKCU}\\Software\\Microsoft\\EdgeUpdate\\ClientState\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}`, '"lastrun"', '"1"');
    const r3 = check(
      `${HKCU}\\Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\Shell\\MuiCache`,
      '"D:\\\\a\\\\rhizomind.exe.FriendlyAppName"',
      '"Rhizomind"',
    );
    for (const r of [r1, r2, r3]) {
      expect(r.violations).toEqual([]);
      expect(r.known).toHaveLength(1);
    }
  });

  it('прочие изменения без упоминания приложения — фон ОС, не нарушение', () => {
    const r = check(`${HKCU}\\Software\\Mozilla\\Firefox`, '"x"', '"1"');
    expect(r.violations).toEqual([]);
    expect(r.background).toHaveLength(1);
  });
});
