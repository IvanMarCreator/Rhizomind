#!/usr/bin/env python3
"""Таблицы классов символов Unicode для разбора тегов (выпуск 0.2, С2-3.3, Р17 п.3–4).

Одни и те же таблицы нужны ядру (Rust, `src-tauri/src/markdown/unicode_tables.rs`)
и редактору (TypeScript, `src/core/tags/unicode-tables.ts`): правила тегов обязаны
совпадать побайтно, а встроенные классы (`char::is_alphabetic`, `\\p{L}`, `\\s`)
зависят от версии Unicode компилятора и браузерного движка. Поэтому оба файла
генерируются этим скриптом из одного источника — файлов базы символов Unicode
(UCD) зафиксированной версии.

Источник: https://www.unicode.org/Public/<версия>/ucd/UnicodeData.txt и PropList.txt
(версия — `UNICODE_VERSION` ниже; SHA-256 файлов проверяется).

Запуск из корня репозитория (папка с UCD — любая, файлы скачиваются вручную):

    mkdir -p /tmp/ucd && cd /tmp/ucd && \\
      curl -sSO https://www.unicode.org/Public/16.0.0/ucd/UnicodeData.txt && \\
      curl -sSO https://www.unicode.org/Public/16.0.0/ucd/PropList.txt
    python3 -I tools/unicode-tables/gen.py /tmp/ucd

Классы:
- PUNCT_SYMBOL — общие категории P* и S* (знаки, допустимые перед `#`; исключения
  `& / \\ # _` — в коде разбора, Р17 п.3);
- WHITE_SPACE — свойство White_Space (PropList.txt, Р17 п.4);
- TAG_WORD — буквы L*, комбинируемые знаки M*, десятичные цифры Nd (тело тега без
  `_ - /`, С2-3.3, Р15);
- DECIMAL_DIGIT — Nd (тег из одних цифр — не тег, Р15).
"""

import hashlib
import os
import sys

UNICODE_VERSION = '16.0.0'
SHA256 = {
    'UnicodeData.txt': 'ff58e5823bd095166564a006e47d111130813dcf8bf234ef79fa51a870edb48f',
    'PropList.txt': '53d614508e2a0b2305a8aa21cd60d993de9326cdf65993660dfcce4503548583',
}

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RUST_OUT = os.path.join(ROOT, 'src-tauri', 'src', 'markdown', 'unicode_tables.rs')
TS_OUT = os.path.join(ROOT, 'src', 'core', 'tags', 'unicode-tables.ts')


def read_checked(ucd, name):
    path = os.path.join(ucd, name)
    with open(path, 'rb') as f:
        data = f.read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != SHA256[name]:
        sys.exit(f'{name}: SHA-256 {digest} не совпадает с ожидаемым для Unicode {UNICODE_VERSION}')
    return data.decode('utf-8')


def general_categories(text):
    """Кодовая точка → общая категория; диапазоны `<…, First>`/`<…, Last>` раскрываются."""
    cats = {}
    first = None
    for line in text.splitlines():
        fields = line.split(';')
        code, name, cat = int(fields[0], 16), fields[1], fields[2]
        if name.endswith(', First>'):
            first = code
            continue
        if name.endswith(', Last>'):
            for c in range(first, code + 1):
                cats[c] = cat
            first = None
            continue
        cats[code] = cat
    return cats


def white_space(text):
    points = set()
    for line in text.splitlines():
        line = line.split('#', 1)[0].strip()
        if not line:
            continue
        codes, prop = [s.strip() for s in line.split(';')]
        if prop != 'White_Space':
            continue
        lo, _, hi = codes.partition('..')
        for c in range(int(lo, 16), int(hi or lo, 16) + 1):
            points.add(c)
    return points


def ranges(points):
    result = []
    for c in sorted(points):
        if result and result[-1][1] + 1 == c:
            result[-1][1] = c
        else:
            result.append([c, c])
    return [tuple(r) for r in result]


TABLES = [
    ('PUNCT_SYMBOL', 'Общие категории P* и S*.', lambda cat: cat[0] in 'PS'),
    ('TAG_WORD', 'Буквы L*, комбинируемые знаки M*, десятичные цифры Nd.', lambda cat: cat[0] in 'LM' or cat == 'Nd'),
    ('DECIMAL_DIGIT', 'Десятичные цифры Nd.', lambda cat: cat == 'Nd'),
]

HEADER = (
    f'Сгенерировано tools/unicode-tables/gen.py из UnicodeData.txt и PropList.txt Unicode {UNICODE_VERSION}.\n'
    'Не править вручную: те же таблицы — в src-tauri/src/markdown/unicode_tables.rs и\n'
    'src/core/tags/unicode-tables.ts (правила тегов в ядре и редакторе совпадают, С2-3.3, Р17).\n'
    'Диапазоны кодовых точек включительно, по возрастанию, без пересечений.'
)


def rust(tables):
    out = [f'//! {line}' if line else '//!' for line in HEADER.split('\n')]
    out += ['', f'/// Версия Unicode, из которой построены таблицы.', f'pub const UNICODE_VERSION: &str = "{UNICODE_VERSION}";', '']
    for name, doc, rs in tables:
        out.append(f'/// {doc}')
        out.append('#[rustfmt::skip]')
        out.append(f'pub static {name}: &[(u32, u32)] = &[')
        for i in range(0, len(rs), 6):
            out.append('    ' + ' '.join(f'(0x{lo:X}, 0x{hi:X}),' for lo, hi in rs[i:i + 6]))
        out.append('];')
        out.append('')
    out += [
        '/// Есть ли кодовая точка в таблице (двоичный поиск).',
        'pub fn contains(table: &[(u32, u32)], c: char) -> bool {',
        '    let c = u32::from(c);',
        '    table',
        '        .binary_search_by(|&(lo, hi)| {',
        '            if hi < c {',
        '                std::cmp::Ordering::Less',
        '            } else if lo > c {',
        '                std::cmp::Ordering::Greater',
        '            } else {',
        '                std::cmp::Ordering::Equal',
        '            }',
        '        })',
        '        .is_ok()',
        '}',
    ]
    return '\n'.join(out) + '\n'


def ts(tables):
    out = [f'// {line}' for line in HEADER.split('\n')]
    out += ['// Таблица — пары [начало, конец] подряд в одном массиве.', '', '/** Версия Unicode, из которой построены таблицы. */', f"export const UNICODE_VERSION = '{UNICODE_VERSION}';", '']
    for name, doc, rs in tables:
        out.append(f'/** {doc} */')
        out.append(f'export const {name}: readonly number[] = [')
        for i in range(0, len(rs), 6):
            out.append('  ' + ' '.join(f'0x{lo:x}, 0x{hi:x},' for lo, hi in rs[i:i + 6]))
        out.append('];')
        out.append('')
    out += [
        '/** Есть ли кодовая точка в таблице (двоичный поиск). */',
        'export function inTable(table: readonly number[], code: number): boolean {',
        '  let lo = 0;',
        '  let hi = table.length / 2 - 1;',
        '  while (lo <= hi) {',
        '    const mid = (lo + hi) >> 1;',
        '    if ((table[mid * 2 + 1] ?? 0) < code) lo = mid + 1;',
        '    else if ((table[mid * 2] ?? 0) > code) hi = mid - 1;',
        '    else return true;',
        '  }',
        '  return false;',
        '}',
    ]
    return '\n'.join(out) + '\n'


def main():
    if len(sys.argv) != 2:
        sys.exit('использование: gen.py <папка с UnicodeData.txt и PropList.txt>')
    ucd = sys.argv[1]
    cats = general_categories(read_checked(ucd, 'UnicodeData.txt'))
    tables = [(name, doc, ranges(c for c, cat in cats.items() if pick(cat))) for name, doc, pick in TABLES]
    ws = ranges(white_space(read_checked(ucd, 'PropList.txt')))
    tables.insert(1, ('WHITE_SPACE', 'Свойство White_Space (PropList.txt).', ws))
    os.makedirs(os.path.dirname(TS_OUT), exist_ok=True)
    for path, body in ((RUST_OUT, rust(tables)), (TS_OUT, ts(tables))):
        with open(path, 'w', encoding='utf-8', newline='\n') as f:
            f.write(body)
        print(path, ', '.join(f'{n}: {len(r)}' for n, _, r in tables))


if __name__ == '__main__':
    main()
