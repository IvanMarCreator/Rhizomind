import { protocol } from 'electron';
import { promises as fs } from 'node:fs';
import path from 'node:path';
import { isInside } from './vault/safe-path';

/**
 * Интерфейс приложения отдаётся через собственный протокол app://rhizomind/.
 * Так интерфейс не имеет доступа к file:// (то есть ко всему диску)
 * и не зависит от localhost-сервера.
 */
export const APP_SCHEME = 'app';
export const APP_HOST = 'rhizomind';
export const APP_ORIGIN = `${APP_SCHEME}://${APP_HOST}`;

/** Жёсткая политика контента: только свои скрипты, никаких внешних подключений. */
export const CONTENT_SECURITY_POLICY = [
  "default-src 'none'",
  "script-src 'self'",
  "style-src 'self'",
  "img-src 'self' data: blob:",
  "font-src 'self'",
  "connect-src 'none'",
  "frame-src 'none'",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'none'",
].join('; ');

const MIME: Record<string, string> = {
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
  protocol.registerSchemesAsPrivileged([
    { scheme: APP_SCHEME, privileges: { standard: true, secure: true } },
  ]);
}

/** Вызывать после app.whenReady(). */
export function handleAppProtocol(rendererRoot: string): void {
  protocol.handle(APP_SCHEME, async (request) => {
    const url = new URL(request.url);
    if (url.host !== APP_HOST) return new Response(null, { status: 404 });

    const rel = decodeURIComponent(url.pathname);
    const target = path.normalize(path.join(rendererRoot, rel === '/' ? 'index.html' : rel));
    if (!isInside(rendererRoot, target)) return new Response(null, { status: 403 });

    try {
      const data = await fs.readFile(target);
      return new Response(new Uint8Array(data), {
        headers: {
          'content-type': MIME[path.extname(target).toLowerCase()] ?? 'application/octet-stream',
          'content-security-policy': CONTENT_SECURITY_POLICY,
          'x-content-type-options': 'nosniff',
        },
      });
    } catch {
      return new Response(null, { status: 404 });
    }
  });
}
