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

/** Находка с позицией в строке (с 1): нужна, чтобы найти исходник по карте сборки. */
export interface LocatedFinding extends Finding {
  column: number;
}

interface Located {
  line: number;
  column: number;
}

interface Node {
  type: string;
  loc?: { start: { line: number; column: number } };
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
const GLOBAL_OBJECTS = new Set(['window', 'globalThis', 'self', 'top', 'parent', 'frames', 'document.defaultView']);

// Элементы разметки Svelte, которые загружают внешнее содержимое, отправляют
// данные или меняют базовый адрес страницы.
const SUSPICIOUS_ELEMENTS = new Set(['iframe', 'object', 'embed', 'script', 'link', 'form', 'base', 'meta']);

// Выполнение строки как кода: так можно собрать любое обращение к сети.
const CODE_FROM_STRING = new Set(['eval', 'Function']);
// Таймеры тоже выполняют строку как код: setTimeout('fetch(u)').
const TIMERS = new Set(['setTimeout', 'setInterval']);
// Объекты, которые загружают адрес из свойства `src`.
const LOADING_CONSTRUCTORS = new Set(['Image', 'Audio']);
// Атрибуты разметки, которые ведут на адрес.
const URL_ATTRIBUTES = new Set(['href', 'src', 'action', 'formaction', 'srcset', 'poster', 'data']);
// Отправка формы — запрос по адресу из action.
const SUBMIT_METHODS = new Set(['submit', 'requestSubmit']);

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

/** `document.defaultView` — тот же глобальный объект, что и `window`. */
function isDefaultView(node: unknown): boolean {
  return (
    isNode(node) &&
    node.type === 'MemberExpression' &&
    memberName(node) === 'defaultView' &&
    (node.object as Node).type === 'Identifier' &&
    (node.object as Node).name === 'document'
  );
}

// Свойства, в которых обычно лежит окно: `el.ownerDocument.defaultView`, `view.win` (CodeMirror).
const WINDOW_PROPERTIES = new Set(['defaultView', 'win']);

/** `X.defaultView` или `X.win` — выражение, которое, скорее всего, окно. */
function isWindowProperty(node: Node): boolean {
  const name = memberName(node);
  return name !== undefined && WINDOW_PROPERTIES.has(name);
}

/** Текст цепочки свойств для сообщения: `this.view.win`, `el.ownerDocument.defaultView`. */
function memberPath(node: Node): string {
  if (node.type === 'Identifier') return node.name as string;
  if (node.type === 'ThisExpression') return 'this';
  if (node.type === 'MemberExpression') {
    const name = memberName(node);
    return `${memberPath(node.object as Node)}.${name ?? '[<вычисляемое>]'}`;
  }
  return '(…)';
}

/** Имя глобального объекта для выражения: идентификатор или `document.defaultView`. */
function globalName(node: unknown): string | undefined {
  if (!isNode(node)) return undefined;
  if (node.type === 'Identifier') return node.name as string;
  if (isDefaultView(node)) return 'document.defaultView';
  return undefined;
}

function aliasDefinition(node: Node): { alias: string; source: string } | undefined {
  let target: unknown;
  let value: unknown;
  if (node.type === 'VariableDeclarator') [target, value] = [node.id, node.init];
  else if (node.type === 'AssignmentExpression' && node.operator === '=') [target, value] = [node.left, node.right];
  else return undefined;
  const source = globalName(value);
  if (isNode(target) && target.type === 'Identifier' && source !== undefined) {
    return { alias: target.name as string, source };
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
  readonly findings: LocatedFinding[] = [];

  private readonly text: string;
  // Глобальные объекты и их псевдонимы в этом файле (`const w = window`).
  private readonly globals: Set<string>;

  constructor(text: string, root: Node) {
    this.text = text;
    this.globals = collectGlobalAliases(root);
  }

  private positionOf(node: Node): Located {
    if (node.loc) return { line: node.loc.start.line, column: node.loc.start.column + 1 };
    if (typeof node.start === 'number') return positionAt(this.text, node.start);
    return { line: 0, column: 0 };
  }

  private report(node: Node, what: string, severity: Severity): void {
    this.findings.push({ ...this.positionOf(node), what, severity });
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
    if (LOADING_CONSTRUCTORS.has(name)) {
      this.report(node, name, 'suspicious');
    } else if (CODE_FROM_STRING.has(name)) {
      // `x instanceof Function` — только проверка типа: Function не вызывается.
      // Отдельный вид, чтобы исключение для него не разрешало другие обращения.
      const typeCheck =
        name === 'Function' &&
        parent?.type === 'BinaryExpression' &&
        parent.operator === 'instanceof' &&
        parent.right === node;
      this.report(node, typeCheck ? 'Function справа от instanceof' : name, 'suspicious');
    } else if (this.globals.has(name) && !isHarmlessGlobalUse(node, parent)) {
      this.report(node, `глобальный объект ${name} без свойства`, 'suspicious');
    }
  }

  private checkMember(node: Node): void {
    const object = node.object as Node;
    const name = memberName(node);
    // navigator['send' + 'Beacon'] — вычисляемое имя у navigator.
    if (object.type === 'Identifier' && object.name === 'navigator' && name === undefined) {
      this.report(node, 'navigator[<вычисляемое>]', 'suspicious');
      return;
    }
    const objectName = globalName(object);
    if (objectName === undefined && isWindowProperty(object)) {
      this.checkWindowMember(node, memberPath(object), name);
      return;
    }
    if (objectName === undefined || !this.globals.has(objectName)) return;
    if (name === undefined) {
      this.report(node, `${objectName}[<вычисляемое>]`, 'suspicious');
    } else if (name === 'open') {
      this.report(node, 'window.open', 'network');
    } else if (node.computed && NETWORK_GLOBALS.has(name)) {
      this.report(node, name, 'network');
    }
  }

  /** Обращение к окну через `X.defaultView` или `X.win` (окно документа, окно редактора). */
  private checkWindowMember(node: Node, objectText: string, name: string | undefined): void {
    if (name === undefined) {
      this.report(node, `${objectText}[<вычисляемое>]`, 'suspicious');
    } else if (name === 'open') {
      this.report(node, 'window.open', 'network');
    } else if (node.computed && NETWORK_GLOBALS.has(name)) {
      this.report(node, name, 'network');
    }
  }

  private checkTimer(node: Node, name: string): void {
    const first = (node.arguments as Node[])[0];
    if (!first) return;
    const isString =
      stringValue(first) !== undefined ||
      first.type === 'TemplateLiteral' ||
      (first.type === 'BinaryExpression' && first.operator === '+');
    if (isString) this.report(node, `${name}(<строка>)`, 'suspicious');
  }

  private checkCreateElement(node: Node): void {
    const first = (node.arguments as Node[])[0];
    // Вычисляемое имя не считается находкой: так создаёт элементы сам Svelte
    // (create_element в собранном интерфейсе). Элементы, загружающие адрес, из
    // разметки ловит проверка <svelte:element> и RegularElement, а сеть — CSP.
    const tag = stringValue(first);
    if (tag !== undefined && SUSPICIOUS_ELEMENTS.has(tag.toLowerCase())) {
      this.report(node, `createElement(${tag.toLowerCase()})`, 'suspicious');
    }
  }

  private checkCall(node: Node): void {
    const callee = node.callee as Node;
    if (callee.type === 'Identifier') {
      const name = callee.name as string;
      if (name === 'require') this.checkModule(node, (node.arguments as Node[])[0], 'require');
      if (name === 'open') this.report(node, 'window.open', 'network');
      if (TIMERS.has(name)) this.checkTimer(node, name);
      return;
    }
    const method = memberName(callee);
    if (method !== undefined && TIMERS.has(method)) this.checkTimer(node, method);
    if (method === 'createElement' || method === 'createElementNS') this.checkCreateElement(node);
    if (method !== undefined && SUBMIT_METHODS.has(method)) this.report(node, 'form.submit', 'network');
    if ((method === 'assign' || method === 'replace') && involvesLocation(callee.object as Node)) {
      this.report(node, `location.${method}`, 'network');
    }
  }

  /** Атрибут разметки Svelte: внешний адрес в href/src/… и `download`. */
  private checkAttribute(node: Node): void {
    const name = (node.name as string).toLowerCase();
    if (name === 'download') {
      this.report(node, 'атрибут download', 'network');
      return;
    }
    if (!URL_ATTRIBUTES.has(name) || !Array.isArray(node.value)) return;
    const text = (node.value as Node[])
      .filter((part) => part.type === 'Text')
      .map((part) => part.data as string)
      .join('');
    if (EXTERNAL_URL.test(text.trim())) this.report(node, `внешний адрес ${text.trim()}`, 'network');
  }

  visit(node: Node, parent?: Node): void {
    switch (node.type) {
      case 'RegularElement':
        if (SUSPICIOUS_ELEMENTS.has(node.name as string)) {
          this.report(node, `элемент <${node.name as string}>`, 'suspicious');
        }
        break;
      case 'SvelteElement':
        this.report(node, 'элемент <svelte:element>', 'suspicious');
        break;
      case 'Attribute':
        this.checkAttribute(node);
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

function positionAt(text: string, offset: number): Located {
  const before = text.slice(0, offset);
  return { line: before.split('\n').length, column: offset - before.lastIndexOf('\n') };
}

function scanScript(text: string, lineOffset = 0, jsx = false): LocatedFinding[] {
  const ast = parseTs(text, { loc: true, range: false, jsx }) as unknown as Node;
  const scanner = new Scanner(text, ast);
  scanner.visit(ast);
  return scanner.findings.map((f) => ({ ...f, line: f.line + lineOffset }));
}

function scanSvelte(text: string): LocatedFinding[] {
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

function scanHtml(text: string): LocatedFinding[] {
  const findings: LocatedFinding[] = [];
  for (const match of text.matchAll(HTML_EXTERNAL_ATTR)) {
    findings.push({ ...positionAt(text, match.index), what: `внешний адрес ${match[1]}`, severity: 'network' });
  }
  for (const match of text.matchAll(HTML_INLINE_SCRIPT)) {
    const body = match[1] ?? '';
    const bodyStart = match.index + match[0].indexOf('>') + 1;
    findings.push(...scanScript(body, lineAt(text, bodyStart) - 1));
  }
  return findings;
}

/** Находки в тексте одного файла с позицией в строке, по возрастанию позиции. Вид файла — по расширению. */
export function scanSourceLocated(text: string, fileName: string): LocatedFinding[] {
  let findings: LocatedFinding[];
  if (fileName.endsWith('.svelte')) findings = scanSvelte(text);
  else if (fileName.endsWith('.html')) findings = scanHtml(text);
  else findings = scanScript(text, 0, /\.[jt]sx$/.test(fileName));
  return findings.sort((a, b) => a.line - b.line || a.column - b.column);
}

/** Находки в тексте одного файла, по возрастанию строки. Вид файла — по расширению. */
export function scanSource(text: string, fileName: string): Finding[] {
  return scanSourceLocated(text, fileName).map(({ line, what, severity }) => ({ line, what, severity }));
}

export interface FileFinding extends LocatedFinding {
  /** Путь относительно base, через «/». */
  file: string;
}

const SOURCE_FILE = /\.(?:ts|tsx|mts|cts|js|jsx|mjs|cjs|svelte|html)$/;
// Модульные тесты в сборку не попадают (их подмены используют глобальные
// объекты); то, что попадает в сборку, проверяется ещё и в dist/.
const TEST_FILE = /\.test\.[cm]?[jt]sx?$/;

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
      let findings: LocatedFinding[];
      try {
        findings = scanSourceLocated(text, file);
      } catch (error) {
        const message = error instanceof Error ? error.message.split('\n')[0] : String(error);
        findings = [{ line: 0, column: 0, what: `не удалось разобрать: ${message}`, severity: 'suspicious' }];
      }
      for (const finding of findings) found.push({ file: relative, ...finding });
    }
  }
  return found.sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line || a.column - b.column);
}
