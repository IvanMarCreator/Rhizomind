// Тест полноты словаря (С3.8): все тексты интерфейса — из словаря `ru`.
// 1. Каждый код ошибки ядра и каждая причина отказа в имени есть в словаре
//    (списки берутся из src/api/types.ts — тех же типов, что сверяются с ядром).
// 2. В разметке .svelte нет текста с буквами и видимых атрибутов с буквами.
// 3. В коде интерфейса (TS и <script> в .svelte) нет строк на русском, кроме
//    технических: текст исключений и консоли (они не показываются пользователю).
import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { parse as parseTs } from '@typescript-eslint/typescript-estree';
import { parse as parseSvelte } from 'svelte/compiler';
import { describe, expect, it } from 'vitest';
import { hasMessage } from './index.ts';

const SRC = path.join(import.meta.dirname, '..');

function unionMembers(source: string, typeName: string): string[] {
  const match = new RegExp(`export type ${typeName} =([^;]+);`).exec(source);
  if (!match?.[1]) throw new Error(`Нет типа ${typeName}`);
  return [...match[1].matchAll(/'([a-z_]+)'/g)].map((m) => m[1] ?? '');
}

const typesSource = readFileSync(path.join(SRC, 'api', 'types.ts'), 'utf8');

describe('словарь покрывает ответы ядра', () => {
  const codes = unionMembers(typesSource, 'ErrorCode');
  const problems = unionMembers(typesSource, 'NameProblem');
  const warnings = unionMembers(typesSource, 'NameWarning');

  it('списки кодов прочитаны из типов API', () => {
    expect(codes).toContain('not_implemented');
    expect(codes.length).toBeGreaterThanOrEqual(14);
    expect(problems).toContain('note_extension');
    expect(problems.length).toBeGreaterThanOrEqual(7);
  });

  it.each(codes)('errors.%s', (code) => {
    expect(hasMessage(`errors.${code}`)).toBe(true);
  });

  it.each(problems)('names.%s', (problem) => {
    expect(hasMessage(`names.${problem}`)).toBe(true);
  });

  it('предупреждения об имени прочитаны из типов API', () => {
    expect(warnings).toEqual(['unlinkable', 'hidden']);
  });

  it.each(warnings)('names.warning.%s', (warning) => {
    expect(hasMessage(`names.warning.${warning}`)).toBe(true);
  });

  it('errors.ipc_failed (отказ моста Tauri)', () => {
    expect(hasMessage('errors.ipc_failed')).toBe(true);
  });
});

// ---- Поиск текстов мимо словаря -------------------------------------------

/** Файлы интерфейса без словаря, тестов и подмены ядра для тестов. */
function sourceFiles(dir: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (full === path.join(SRC, 'i18n')) continue;
      files.push(...sourceFiles(full));
    } else if (/\.(ts|svelte)$/.test(entry.name) && !/\.test\.ts$|\.d\.ts$/.test(entry.name)) {
      if (full === path.join(SRC, 'api', 'testing.ts')) continue;
      files.push(full);
    }
  }
  return files;
}

type AnyNode = { type: string; [key: string]: unknown };

function isNode(value: unknown): value is AnyNode {
  return typeof value === 'object' && value !== null && typeof (value as AnyNode).type === 'string';
}

const LETTER = /\p{L}/u;
const CYRILLIC = /\p{Script=Cyrillic}/u;
/** Атрибуты, которые видит пользователь (подсказки, чтение с экрана). */
const VISIBLE_ATTRIBUTES = new Set([
  'title',
  'alt',
  'placeholder',
  'label',
  'aria-label',
  'aria-description',
  'aria-placeholder',
  'aria-roledescription',
  'aria-valuetext',
]);

function calleeName(node: AnyNode): string {
  const callee = node.callee as AnyNode | undefined;
  if (!callee) return '';
  if (callee.type === 'Identifier') return String(callee.name);
  if (callee.type === 'Super') return 'super';
  if (callee.type === 'MemberExpression' && isNode(callee.object) && callee.object.type === 'Identifier') {
    return `${String(callee.object.name)}.`;
  }
  return '';
}

/** Строка — техническая: исключение, вызов конструктора ошибки, super(), консоль. */
function isTechnical(ancestors: AnyNode[]): boolean {
  return ancestors.some((node) => {
    if (node.type === 'ThrowStatement') return true;
    if (node.type === 'NewExpression') return /Error$|Mismatch$|TooLarge$/.test(calleeName(node));
    if (node.type === 'CallExpression') return calleeName(node) === 'super' || calleeName(node) === 'console.';
    return false;
  });
}

interface Leak {
  file: string;
  text: string;
}

function walk(node: unknown, ancestors: AnyNode[], visit: (node: AnyNode, ancestors: AnyNode[]) => void): void {
  if (Array.isArray(node)) {
    for (const child of node) walk(child, ancestors, visit);
    return;
  }
  if (!isNode(node)) return;
  visit(node, ancestors);
  const next = [...ancestors, node];
  for (const [key, value] of Object.entries(node)) {
    if (key === 'parent' || key === 'loc' || key === 'range') continue;
    if (typeof value === 'object' && value !== null) walk(value, next, visit);
  }
}

function scriptLeaks(file: string, root: unknown): Leak[] {
  const leaks: Leak[] = [];
  walk(root, [], (node, ancestors) => {
    let value: string | undefined;
    if (node.type === 'Literal' && typeof node.value === 'string') value = node.value;
    if (node.type === 'TemplateElement') value = (node.value as { cooked?: string }).cooked ?? undefined;
    if (value && CYRILLIC.test(value) && !isTechnical(ancestors)) leaks.push({ file, text: value });
  });
  return leaks;
}

function markupLeaks(file: string, fragment: unknown): Leak[] {
  const leaks: Leak[] = [];
  walk(fragment, [], (node, ancestors) => {
    // Выражения в разметке — код: строки в них проверяет scriptLeaks.
    if (ancestors.some((a) => a.type === 'ExpressionTag' || a.type === 'Script')) return;
    if (node.type === 'Text' && LETTER.test(String(node.data))) {
      const insideAttribute = ancestors.some((a) => a.type === 'Attribute');
      const attribute = ancestors.findLast((a) => a.type === 'Attribute');
      if (!insideAttribute) leaks.push({ file, text: String(node.data).trim() });
      else if (attribute && VISIBLE_ATTRIBUTES.has(String(attribute.name))) {
        leaks.push({ file, text: `${String(attribute.name)}="${String(node.data)}"` });
      }
    }
  });
  return leaks;
}

function leaksIn(file: string): Leak[] {
  const text = readFileSync(file, 'utf8');
  const relative = path.relative(SRC, file).split(path.sep).join('/');
  if (file.endsWith('.ts')) return scriptLeaks(relative, parseTs(text, { range: false, loc: false }));
  const ast = parseSvelte(text, { modern: true }) as unknown as AnyNode;
  return [
    ...markupLeaks(relative, ast.fragment),
    ...scriptLeaks(relative, ast.instance),
    ...scriptLeaks(relative, ast.module),
    // Строки в выражениях разметки: {cond ? 'да' : 'нет'}.
    ...scriptLeaks(relative, collectExpressions(ast.fragment)),
  ];
}

function collectExpressions(fragment: unknown): unknown[] {
  const expressions: unknown[] = [];
  walk(fragment, [], (node, ancestors) => {
    if (ancestors.some((a) => a.type === 'Script')) return;
    if ('expression' in node && isNode(node.expression)) expressions.push(node.expression);
  });
  return expressions;
}

describe('нет текстов мимо словаря', () => {
  it('поиск находит текст в разметке, видимых атрибутах и строках кода', () => {
    const sample = `<script lang="ts">const label = 'Привет';</script>
<p>Текст</p><button title="Подсказка" class="кнопка">{ok ? 'Да' : t('x')}</button>
<svg viewBox="0 0 24 24"><path d="M5 12h14" /></svg>`;
    const ast = parseSvelte(sample, { modern: true }) as unknown as AnyNode;
    const found = [
      ...markupLeaks('x', ast.fragment),
      ...scriptLeaks('x', ast.instance),
      ...scriptLeaks('x', collectExpressions(ast.fragment)),
    ].map((leak) => leak.text);

    expect(found.sort()).toEqual(['Да', 'Привет', 'Текст', 'title="Подсказка"'].sort());
  });

  it('технические строки (исключения, консоль) не считаются', () => {
    const code = `throw new Error('Нет элемента');
class X extends Error { constructor() { super('Описание'); } }
console.error('Сбой');
const shown = 'Показано';`;
    const found = scriptLeaks('x', parseTs(code, { range: false, loc: false })).map((leak) => leak.text);

    expect(found).toEqual(['Показано']);
  });

  it('в src/ (кроме словаря и тестов) все тексты — из словаря', () => {
    const files = sourceFiles(SRC);
    expect(files.some((file) => file.endsWith('.svelte'))).toBe(true);

    const leaks = files.flatMap(leaksIn);

    expect(leaks).toEqual([]);
  });
});
