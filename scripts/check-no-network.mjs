// Проверка для ИБ: в исходниках приложения (src/) нет сетевого кода.
// Запуск: node scripts/check-no-network.mjs [папка]  (по умолчанию src/).
// Код выхода 1 и список «файл:строка  что найдено», если сеть найдена.
import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const NODE_NET_MODULES = 'http|https|http2|net|tls|dgram|dns';

/** Правила: регулярное выражение (с флагом g) и название находки. */
const RULES = [
  [new RegExp(String.raw`\bfrom\s*['"](?:node:)?(${NODE_NET_MODULES})['"]`, 'g'), '$1'],
  [new RegExp(String.raw`\b(?:require|import)\s*\(\s*['"](?:node:)?(${NODE_NET_MODULES})['"]\s*\)`, 'g'), '$1'],
  [/\bimport\s*(?:type\s*)?\{[^}]*\bnet\b[^}]*\}\s*from\s*['"]electron['"]/g, 'electron.net'],
  [/\belectron\s*\)?\s*\.\s*net\b/g, 'electron.net'],
  [/\bfetch\s*\(/g, 'fetch('],
  [/\bXMLHttpRequest\b/g, 'XMLHttpRequest'],
  [/\bWebSocket\b/g, 'WebSocket'],
  [/\bEventSource\b/g, 'EventSource'],
  [/\bsendBeacon\b/g, 'navigator.sendBeacon'],
  [/\bautoUpdater\b|\belectron-updater\b/g, 'autoUpdater'],
  [/\bcrashReporter\b/g, 'crashReporter'],
];

const SOURCE_FILE = /\.(?:ts|tsx|js|mjs|cjs|svelte|html)$/;

/** Находки в тексте одного файла: [{ line, kind }] без повторов. */
export function scanText(text) {
  const found = new Map();
  for (const [re, kind] of RULES) {
    for (const match of text.matchAll(re)) {
      const line = text.slice(0, match.index).split('\n').length;
      const name = kind.startsWith('$') ? match[1] : kind;
      found.set(`${line}:${name}`, { line, kind: name });
    }
  }
  return [...found.values()].sort((a, b) => a.line - b.line);
}

async function* walk(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) yield* walk(full);
    else if (SOURCE_FILE.test(entry.name)) yield full;
  }
}

/** Все находки в папке: [{ file (относительно папки), line, kind }]. */
export async function scanDirectory(dir) {
  const problems = [];
  for await (const file of walk(dir)) {
    const text = await readFile(file, 'utf8');
    for (const hit of scanText(text)) problems.push({ file: path.relative(dir, file), ...hit });
  }
  return problems;
}

async function main() {
  const dir = path.resolve(process.argv[2] ?? path.join(import.meta.dirname, '..', 'src'));
  const problems = await scanDirectory(dir);
  if (problems.length > 0) {
    console.error(
      'Найден сетевой код, запрещённый в Secure-редакции (инвариант 1 в CLAUDE.md):\n' +
        problems.map((p) => `  ${p.file}:${p.line}  ${p.kind}`).join('\n') +
        '\nУберите эти места: в приложении не должно быть сетевых возможностей.',
    );
    process.exit(1);
  }
  console.log(`check:network — сетевой код не найден (${path.relative(process.cwd(), dir) || '.'})`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) await main();
