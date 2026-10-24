// Проверка для ИБ: в исходниках приложения нет сетевого кода.
// Запускается автоматически перед каждой сборкой (npm run build).
// Если что-то найдено, сборка останавливается.
import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';

const ROOT = path.resolve(import.meta.dirname, '..', 'src');

const FORBIDDEN = [
  [/from\s+['"](node:)?(http|https|http2|net|tls|dgram|dns)['"]/, 'импорт сетевого модуля Node.js'],
  [/require\(\s*['"](node:)?(http|https|http2|net|tls|dgram|dns)['"]\s*\)/, 'require сетевого модуля Node.js'],
  [/\bnet\.(request|fetch)\b/, 'сетевой запрос Electron (net)'],
  [/\bfetch\s*\(/, 'fetch()'],
  [/\bXMLHttpRequest\b/, 'XMLHttpRequest'],
  [/\bWebSocket\b/, 'WebSocket'],
  [/\bEventSource\b/, 'EventSource'],
  [/\bsendBeacon\b/, 'navigator.sendBeacon'],
  [/\bautoUpdater\b|electron-updater/, 'автообновление'],
  [/\bshell\.openExternal\b/, 'открытие внешних ссылок'],
];

async function* walk(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) yield* walk(full);
    else if (/\.(ts|tsx|js|mjs|cjs|html)$/.test(entry.name)) yield full;
  }
}

const problems = [];
for await (const file of walk(ROOT)) {
  const lines = (await readFile(file, 'utf8')).split('\n');
  lines.forEach((line, i) => {
    for (const [re, what] of FORBIDDEN) {
      if (re.test(line)) problems.push(`${path.relative(ROOT, file)}:${i + 1}  ${what}`);
    }
  });
}

if (problems.length) {
  console.error('Найден сетевой код, запрещённый в Secure-редакции:\n' + problems.map((p) => '  ' + p).join('\n'));
  process.exit(1);
}
console.log('check:network — сетевой код не найден');
