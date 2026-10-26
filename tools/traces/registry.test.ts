import { describe, expect, it } from 'vitest';
import { checkRegistryChanges, diffRegistry, findMarkers, parseRegExport, searchableText } from './registry.ts';

const HKCU = 'HKEY_CURRENT_USER';

function exportText(body: string): string {
  return `Windows Registry Editor Version 5.00\r\n\r\n${body}`;
}

/** Строка hex:… с текстом в UTF-16LE (так в реестре лежат PIDL и пути); `shift` байт впереди. */
function hexUtf16(text: string, shift = 0): string {
  const bytes = [...Buffer.alloc(shift, 0x14), ...Buffer.from(text, 'utf16le')];
  return 'hex:' + bytes.map((b) => b.toString(16).padStart(2, '0')).join(',');
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

  it('находит строку UTF-16, которая начинается с нечётного байта (PIDL)', () => {
    expect(searchableText(hexUtf16('trace-vault-ab12', 1))).toContain('trace-vault-ab12');
  });
});

describe('checkRegistryChanges', () => {
  const markers = { app: ['rhizomind', 'rhizomind-webview-123-abc'], data: ['trace-vault-ab12', 'заметка-cd34'] };
  const one = (change: Parameters<typeof checkRegistryChanges>[0][number]) => checkRegistryChanges([change], markers);
  const check = (key: string, name: string, data: string) => one({ kind: 'added', key, name, data });
  const explorer = `${HKCU}\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer`;

  it('любая правка ComDlg32, RecentDocs, TypedPaths, CIDOpen, ShellBags — нарушение', () => {
    for (const key of [
      `${explorer}\\ComDlg32\\CIDSizeMRU`,
      `${explorer}\\RecentDocs`,
      `${explorer}\\TypedPaths`,
      `${explorer}\\CIDOpen\\Modules\\GlobalSettings`,
      `${HKCU}\\Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\Shell\\BagMRU\\0`,
      `${HKCU}\\Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\Shell\\Bags\\1\\ComDlg`,
    ]) {
      expect(check(key, '"0"', 'hex:00').violations, key).toHaveLength(1);
    }
  });

  it('упоминание приложения в чужом ключе — нарушение, в том числе внутри двоичного значения', () => {
    expect(check(`${HKCU}\\Software\\Foo`, '"Path"', '"D:\\\\x\\\\Rhizomind\\\\vault"').violations).toHaveLength(1);
    expect(check(`${HKCU}\\Software\\Foo`, '"Bin"', hexUtf16('rhizomind-webview-123-abc')).violations).toHaveLength(1);
  });

  it('метка в имени ключа — нарушение', () => {
    expect(one({ kind: 'added', key: `${HKCU}\\Software\\Foo\\trace-vault-ab12` }).violations).toHaveLength(1);
  });

  it('кириллическая метка в пути внутри двоичного значения — нарушение', () => {
    expect(check(`${HKCU}\\Software\\Foo`, '"Bin"', hexUtf16('D:\\v\\trace-vault-ab12\\заметка-cd34.md', 1)).violations).toHaveLength(1);
    expect(check(`${HKCU}\\Software\\Foo`, '"Str"', '"D:\\\\v\\\\заметка-cd34.md"').violations).toHaveLength(1);
  });

  it('удалённое значение с меткой — нарушение', () => {
    expect(one({ kind: 'removed', key: `${HKCU}\\Software\\Foo`, name: '"x"', data: '"trace-vault-ab12"' }).violations).toHaveLength(1);
  });

  it('метка данных в учтённом ключе движка — нарушение', () => {
    const r = check(`${HKCU}\\Software\\Microsoft\\EdgeWebView\\BLBeacon`, '"x"', '"C:\\\\trace-vault-ab12"');
    expect(r.violations).toHaveLength(1);
    expect(r.known).toEqual([]);
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

describe('findMarkers', () => {
  it('ищет метки данных во всём экспорте, включая учтённые ключи и двоичные значения', () => {
    const reg = parseRegExport(
      exportText(
        `[${HKCU}\\Software\\Microsoft\\EdgeWebView]\r\n"a"="trace-vault-ab12"\r\n` +
          `[${HKCU}\\Software\\Foo]\r\n"b"=${hexUtf16('заметка-cd34', 1)}\r\n"c"="clean"\r\n`,
      ),
    );
    const hits = findMarkers(reg, ['trace-vault-ab12', 'заметка-cd34']).map((h) => `${h.key} ${h.name} ${h.marker}`);
    expect(hits).toEqual([
      `${HKCU}\\Software\\Microsoft\\EdgeWebView "a" trace-vault-ab12`,
      `${HKCU}\\Software\\Foo "b" заметка-cd34`,
    ]);
  });
});
