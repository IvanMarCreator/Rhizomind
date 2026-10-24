import path from 'node:path';

export const APP_SCHEME = 'app';
export const APP_HOST = 'rhizomind';
export const APP_ORIGIN = `${APP_SCHEME}://${APP_HOST}`;

/**
 * Путь к файлу интерфейса для адреса app://rhizomind/…
 * null — если адрес чужой, кодировка битая или путь выходит за папку интерфейса.
 */
export function resolveAppAsset(rendererRoot: string, requestUrl: string): string | null {
  let url: URL;
  let relative: string;
  try {
    url = new URL(requestUrl);
    relative = decodeURIComponent(url.pathname);
  } catch {
    return null;
  }
  if (url.protocol !== `${APP_SCHEME}:` || url.host !== APP_HOST) return null;

  const root = path.resolve(rendererRoot);
  const target = path.resolve(root, '.' + path.sep + (relative === '/' ? 'index.html' : relative));
  const inside = path.relative(root, target);
  if (inside === '' || inside.startsWith('..') || path.isAbsolute(inside)) return null;
  return target;
}
