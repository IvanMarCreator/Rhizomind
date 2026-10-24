// Поиск сетевого кода в исходниках и в собранном интерфейсе — по синтаксическому
// дереву (ESTree), а не регулярными выражениями: строки и комментарии не дают
// ложных срабатываний, а обращения через скобки (window['fe' + 'tch']) не
// проходят незамеченными (ADR 0009).
import { readdir, readFile, stat } from 'node:fs/promises';
import path from 'node:path';
import { parse as parseTs } from '@typescript-eslint/typescript-estree';
import { parse as parseSvelte } from 'svelte/compiler';

export type Severity = 'network' | 'suspicious';

export interface Finding {
  line: number;
  what: string;
  severity: Severity;
}

interface Node {
  type: string;
  loc?: { start: { line: number } };
  start?: number;
  [key: string]: unknown;
}

// Имена сетевых API браузера. Ищутся в любом месте дерева, где стоит
// идентификатор: вызов, `new`, свойство объекта (`window.fetch`), ключ
// деструктуризации (`const { fetch } = window`). Своё свойство с таким же
// именем (`store.fetch`) тоже считается находкой: его нужно переименовать —
// так проверка не зависит от того, через какой объект идёт обращение.
const NETWORK_GLOBALS = new Set([
  'fetch',
  'Request',
  'XMLHttpRequest',
  'WebSocket',
  'WebSocketStream',
  'EventSource',
  'RTCPeerConnection',
  'webkitRTCPeerConnection',
  'RTCDataChannel',
  'WebTransport',
  'importScripts',
  'sendBeacon',
  // Фоновые потоки могут загрузить скрипт по адресу.
  'Worker',
  'SharedWorker',
  'serviceWorker',
]);

// Глобальный объект: через него можно добраться до любого API по имени.
const GLOBAL_OBJECTS = new Set(['window', 'globalThis', 'self', 'top', 'parent', 'frames']);

// Элементы разметки Svelte, которые загружают внешнее содержимое, отправляют
// данные или меняют базовый адрес страницы.
const SUSPICIOUS_ELEMENTS = new Set(['iframe', 'object', 'embed', 'script', 'link', 'form', 'base', 'meta']);

// Выполнение строки как кода: так можно собрать любое обращение к сети.
const CODE_FROM_STRING = new Set(['eval', 'Function']);

// Модули Node с сетью (в любом виде: `http`, `node:http`, `node:dns/promises`)
// и пакеты с сетевым кодом, включая сетевые плагины Tauri и opener/shell
// (открытие адреса без подтверждения, ADR 0009).
const NETWORK_MODULES = new Set(['http', 'https', 'http2', 'net', 'tls', 'dgram', 'dns', 'undici', 'ws']);
const NETWORK_PACKAGES = new Set([
  '@tauri-apps/plugin-http',
  '@tauri-apps/plugin-websocket',
  '@tauri-apps/plugin-upload',
  '@tauri-apps/plugin-updater',
  '@tauri-apps/plugin-opener',
  '@tauri-apps/plugin-shell',
  '@tauri-apps/plugin-localhost',
]);
const EXTERNAL_URL = /^(?:https?:|wss?:|\/\/)/i;

function isNetworkModule(specifier: string): boolean {
  if (EXTERNAL_URL.test(specifier)) return true;
  if (specifier.startsWith('@')) {
    const pkg = specifier.split('/').slice(0, 2).join('/');
    return NETWORK_PACKAGES.has(pkg);
  }
  const bare = specifier.replace(/^node:/, '').split('/')[0] ?? '';
  return NETWORK_MODULES.has(bare);
}

function stringValue(node: unknown): string | undefined {
  if (!isNode(node)) return undefined;
  if (node.type === 'Literal' && typeof node.value === 'string') return node.value;
  if (node.type === 'TemplateLiteral' && (node.expressions as unknown[]).length === 0) {
    const [quasi] = node.quasis as { value: { cooked: string } }[];
    return quasi?.value.cooked;
  }
  return undefined;
}

/** Идентификатор стоит там, где он обозначает значение, а не имя свойства или объявления. */
function isReference(node: Node, parent: Node | undefined): boolean {
  if (!parent) return true;
  switch (parent.type) {
    case 'MemberExpression':
      return parent.object === node || parent.computed === true;
    case 'Property':
    case 'PropertyDefinition':
    case 'MethodDefinition':
      return parent.value === node || (parent.key === node && parent.computed === true);
    case 'VariableDeclarator':
      return parent.init === node;
    case 'FunctionDeclaration':
    case 'FunctionExpression':
    case 'ArrowFunctionExpression':
      return parent.body === node;
    case 'ImportSpecifier':
    case 'ImportDefaultSpecifier':
    case 'ImportNamespaceSpecifier':
    case 'ExportSpecifier':
    case 'LabeledStatement':
    case 'BreakStatement':
    case 'ContinueStatement':
    case 'TSTypeReference':
    case 'TSQualifiedName':
    case 'TSPropertySignature':
    case 'TSInterfaceDeclaration':
    case 'TSTypeAliasDeclaration':
      return false;
    default:
      return true;
  }
}

function memberName(node: Node): string | undefined {
  if (node.type !== 'MemberExpression') return undefined;
  const property = node.property as Node;
  if (node.computed) return stringValue(property);
  return property.type === 'Identifier' ? (property.name as string) : undefined;
}

/** Выражение указывает на location или на его часть: `location`, `x.location`, `location.href`. */
function involvesLocation(node: Node): boolean {
  if (node.type === 'Identifier') return node.name === 'location';
  if (node.type === 'MemberExpression') {
    return memberName(node) === 'location' || involvesLocation(node.object as Node);
  }
  return false;
}

function isNode(value: unknown): value is Node {
  return typeof value === 'object' && value !== null && typeof (value as Node).type === 'string';
}

function children(node: Node): Node[] {
  const out: Node[] = [];
  for (const [key, value] of Object.entries(node)) {
    if (key === 'parent' || key === 'loc' || key === 'range') continue;
    if (Array.isArray(value)) out.push(...value.filter(isNode));
    else if (isNode(value)) out.push(value);
  }
  return out;
}

function aliasDefinition(node: Node): { alias: string; source: string } | undefined {
  let target: unknown;
  let value: unknown;
  if (node.type === 'VariableDeclarator') [target, value] = [node.id, node.init];
  else if (node.type === 'AssignmentExpression' && node.operator === '=') [target, value] = [node.left, node.right];
  else return undefined;
  if (isNode(target) && target.type === 'Identifier' && isNode(value) && value.type === 'Identifier') {
    return { alias: target.name as string, source: value.name as string };
  }
  return undefined;
}

/** Глобальные объекты и все имена, которым они присвоены (с цепочками `a = window; b = a`). */
function collectGlobalAliases(root: Node): Set<string> {
  const definitions: { alias: string; source: string }[] = [];
  const stack = [root];
  while (stack.length > 0) {
    const node = stack.pop() as Node;
    const definition = aliasDefinition(node);
    if (definition) definitions.push(definition);
    stack.push(...children(node));
  }
  const globals = new Set(GLOBAL_OBJECTS);
  let grew = true;
  while (grew) {
    grew = false;
    for (const { alias, source } of definitions) {
      if (globals.has(source) && !globals.has(alias)) {
        globals.add(alias);
        grew = true;
      }
    }
  }
  return globals;
}

const COMPARISON = new Set(['===', '!==', '==', '!=']);

/**
 * Использования глобального объекта, которые не открывают доступ к его
 * свойствам по вычисляемому имени: обращение к свойству (проверяется отдельно),
 * typeof, сравнение и заведение псевдонима (псевдоним проверяется как сам объект).
 */
function isHarmlessGlobalUse(node: Node, parent: Node | undefined): boolean {
  if (!parent) return false;
  switch (parent.type) {
    case 'MemberExpression':
      return parent.object === node;
    case 'UnaryExpression':
      return parent.operator === 'typeof';
    case 'BinaryExpression':
      return COMPARISON.has(parent.operator as string);
    case 'VariableDeclarator':
      return parent.init === node && (parent.id as Node).type === 'Identifier';
    case 'AssignmentExpression':
      return (
        parent.left === node ||
        (parent.operator === '=' && parent.right === node && (parent.left as Node).type === 'Identifier')
      );
    default:
      return false;
  }
}

class Scanner {
  readonly findings: Finding[] = [];

  private readonly text: string;
  // Глобальные объекты и их псевдонимы в этом файле (`const w = window`).
  private readonly globals: Set<string>;

  constructor(text: string, root: Node) {
    this.text = text;
    this.globals = collectGlobalAliases(root);
  }

  private lineOf(node: Node): number {
    if (node.loc) return node.loc.start.line;
    if (typeof node.start === 'number') return this.text.slice(0, node.start).split('\n').length;
    return 0;
  }

  private report(node: Node, what: string, severity: Severity): void {
    this.findings.push({ line: this.lineOf(node), what, severity });
  }

  private checkModule(node: Node, source: unknown, how: 'import' | 'require'): void {
    const specifier = stringValue(source);
    if (specifier === undefined) {
      this.report(node, `${how}(<вычисляемое>)`, 'suspicious');
    } else if (isNetworkModule(specifier)) {
      this.report(node, `import ${specifier}`, 'network');
    }
  }

  private checkIdentifier(node: Node, parent: Node | undefined): void {
    const name = node.name as string;
    if (NETWORK_GLOBALS.has(name)) {
      this.report(node, name, 'network');
      return;
    }
    if (!isReference(node, parent)) return;
    if (CODE_FROM_STRING.has(name)) {
      this.report(node, name, 'suspicious');
    } else if (this.globals.has(name) && !isHarmlessGlobalUse(node, parent)) {
      this.report(node, `глобальный объект ${name} без свойства`, 'suspicious');
    }
  }

  private checkMember(node: Node): void {
    const object = node.object as Node;
    if (object.type !== 'Identifier' || !this.globals.has(object.name as string)) return;
    const name = memberName(node);
    if (name === undefined) {
      this.report(node, `${object.name as string}[<вычисляемое>]`, 'suspicious');
    } else if (name === 'open') {
      this.report(node, 'window.open', 'network');
    } else if (node.computed && NETWORK_GLOBALS.has(name)) {
      this.report(node, name, 'network');
    }
  }

  private checkCall(node: Node): void {
    const callee = node.callee as Node;
    if (callee.type === 'Identifier') {
      if (callee.name === 'require') this.checkModule(node, (node.arguments as Node[])[0], 'require');
      if (callee.name === 'open') this.report(node, 'window.open', 'network');
      return;
    }
    const method = memberName(callee);
    if ((method === 'assign' || method === 'replace') && involvesLocation(callee.object as Node)) {
      this.report(node, `location.${method}`, 'network');
    }
  }

  visit(node: Node, parent?: Node): void {
    switch (node.type) {
      case 'RegularElement':
        if (SUSPICIOUS_ELEMENTS.has(node.name as string)) {
          this.report(node, `элемент <${node.name as string}>`, 'suspicious');
        }
        break;
      case 'Identifier':
        this.checkIdentifier(node, parent);
        break;
      case 'MemberExpression':
        this.checkMember(node);
        break;
      case 'AssignmentExpression':
        if (involvesLocation(node.left as Node)) this.report(node, 'присваивание location', 'network');
        break;
      case 'ImportDeclaration':
      case 'ImportExpression':
        this.checkModule(node, node.source, 'import');
        break;
      case 'ExportNamedDeclaration':
      case 'ExportAllDeclaration':
        if (node.source) this.checkModule(node, node.source, 'import');
        break;
      case 'CallExpression':
        this.checkCall(node);
        break;
    }
    for (const child of children(node)) this.visit(child, node);
  }
}

function lineAt(text: string, offset: number): number {
  return text.slice(0, offset).split('\n').length;
}

function scanScript(text: string, lineOffset = 0): Finding[] {
  const ast = parseTs(text, { loc: true, range: false, jsx: false }) as unknown as Node;
  const scanner = new Scanner(text, ast);
  scanner.visit(ast);
  return scanner.findings.map((f) => ({ ...f, line: f.line + lineOffset }));
}

function scanSvelte(text: string): Finding[] {
  const root = parseSvelte(text, { modern: true }) as unknown as Node;
  const scanner = new Scanner(text, root);
  // Код: <script>, <script module> и выражения в разметке. Стили не код.
  for (const part of [root.instance, root.module, root.fragment]) {
    if (isNode(part)) scanner.visit(part);
  }
  return scanner.findings;
}

// В HTML (index.html и собранный интерфейс) — внешние адреса в атрибутах и
// код встроенных скриптов. Файл мал и пишется нами или Vite, поэтому хватает
// разбора по тегам.
const HTML_EXTERNAL_ATTR = /\b(?:src|href|action|formaction|data|poster|srcset)\s*=\s*["']?((?:https?:|wss?:|\/\/)[^"'\s>]*)/gi;
const HTML_INLINE_SCRIPT = /<script\b(?![^>]*\bsrc\s*=)[^>]*>([\s\S]*?)<\/script>/gi;

function scanHtml(text: string): Finding[] {
  const findings: Finding[] = [];
  for (const match of text.matchAll(HTML_EXTERNAL_ATTR)) {
    findings.push({ line: lineAt(text, match.index), what: `внешний адрес ${match[1]}`, severity: 'network' });
  }
  for (const match of text.matchAll(HTML_INLINE_SCRIPT)) {
    const body = match[1] ?? '';
    const bodyStart = match.index + match[0].indexOf('>') + 1;
    findings.push(...scanScript(body, lineAt(text, bodyStart) - 1));
  }
  return findings;
}

/** Находки в тексте одного файла, по возрастанию строки. Вид файла — по расширению. */
export function scanSource(text: string, fileName: string): Finding[] {
  let findings: Finding[];
  if (fileName.endsWith('.svelte')) findings = scanSvelte(text);
  else if (fileName.endsWith('.html')) findings = scanHtml(text);
  else findings = scanScript(text);
  return findings.sort((a, b) => a.line - b.line);
}

export interface FileFinding extends Finding {
  /** Путь относительно base, через «/». */
  file: string;
}

const SOURCE_FILE = /\.(?:ts|mts|cts|js|mjs|cjs|svelte|html)$/;
// Модульные тесты в сборку не попадают (их подмены используют глобальные
// объекты); то, что попадает в сборку, проверяется ещё и в dist/.
const TEST_FILE = /\.test\.[cm]?[jt]s$/;

async function* walk(target: string): AsyncGenerator<string> {
  const info = await stat(target);
  if (info.isFile()) {
    if (SOURCE_FILE.test(target) && !TEST_FILE.test(target)) yield target;
    return;
  }
  const entries = await readdir(target, { withFileTypes: true });
  entries.sort((a, b) => a.name.localeCompare(b.name));
  for (const entry of entries) {
    if (entry.name === 'node_modules') continue;
    yield* walk(path.join(target, entry.name));
  }
}

/** Все находки в файлах и папках; пути в ответе — относительно base. */
export async function scanPaths(targets: string[], base: string): Promise<FileFinding[]> {
  const found: FileFinding[] = [];
  for (const target of targets) {
    for await (const file of walk(target)) {
      const relative = path.relative(base, file).split(path.sep).join('/');
      const text = await readFile(file, 'utf8');
      let findings: Finding[];
      try {
        findings = scanSource(text, file);
      } catch (error) {
        const message = error instanceof Error ? error.message.split('\n')[0] : String(error);
        findings = [{ line: 0, what: `не удалось разобрать: ${message}`, severity: 'suspicious' }];
      }
      for (const finding of findings) found.push({ file: relative, ...finding });
    }
  }
  return found.sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line);
}
