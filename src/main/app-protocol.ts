import { protocol } from 'electron';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { APP_SCHEME, resolveAppAsset } from './app-asset-path';

// Интерфейс отдаётся через собственный протокол app://rhizomind/.
// Так у интерфейса нет доступа к file:// (ко всему диску) и нет localhost-сервера.

const MIME: Readonly<Record<string, string>> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.woff2': 'font/woff2',
  '.json': 'application/json',
};

/** Вызывать до app.whenReady(). */
export function registerAppScheme(): void {
  protocol.registerSchemesAsPrivileged([{ scheme: APP_SCHEME, privileges: { standard: true, secure: true } }]);
}

/** Вызывать после app.whenReady(): отдаёт файлы только из папки интерфейса. */
export function handleAppProtocol(rendererRoot: string): void {
  protocol.handle(APP_SCHEME, async (request) => {
    const target = resolveAppAsset(rendererRoot, request.url);
    if (target === null) return new Response(null, { status: 404 });
    try {
      const data = await readFile(target);
      return new Response(new Uint8Array(data), {
        headers: {
          'content-type': MIME[path.extname(target).toLowerCase()] ?? 'application/octet-stream',
          'x-content-type-options': 'nosniff',
        },
      });
    } catch {
      return new Response(null, { status: 404 });
    }
  });
}
