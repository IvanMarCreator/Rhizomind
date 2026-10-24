import { app, session } from 'electron';
import { POLICY } from '../../shared/edition';
import { APP_SCHEME } from '../app-protocol';

/**
 * «Сетевой выключатель» Rhizomind.
 *
 * Всё, что здесь настроено, действует на весь интерфейс приложения:
 * любые запросы, кроме собственного протокола app:// и встроенных данных
 * (data:, blob:), отменяются до того, как покинут процесс.
 */
const ALLOWED_PROTOCOLS = new Set([`${APP_SCHEME}:`, 'data:', 'blob:', 'devtools:']);

export function applyLockdown(): void {
  const ses = session.defaultSession;

  if (!POLICY.network) {
    ses.webRequest.onBeforeRequest((details, callback) => {
      let allowed = false;
      try {
        allowed = ALLOWED_PROTOCOLS.has(new URL(details.url).protocol);
      } catch {
        allowed = false;
      }
      callback({ cancel: !allowed });
    });
  }

  // Никаких разрешений: камера, микрофон, геолокация, уведомления, буфер и т.д.
  ses.setPermissionRequestHandler((_wc, _permission, callback) => callback(false));
  ses.setPermissionCheckHandler(() => false);

  // Проверка орфографии в Chromium скачивает словари из интернета — выключаем.
  ses.setSpellCheckerEnabled(false);

  // Загрузки файлов из интерфейса запрещены.
  ses.on('will-download', (event) => event.preventDefault());

  app.on('web-contents-created', (_event, contents) => {
    // Запрет навигации со страницы приложения куда-либо ещё.
    contents.on('will-navigate', (event, url) => {
      if (!url.startsWith(`${APP_SCHEME}://`)) event.preventDefault();
    });
    contents.on('will-redirect', (event, url) => {
      if (!url.startsWith(`${APP_SCHEME}://`)) event.preventDefault();
    });
    // Запрет новых окон и <webview>.
    contents.setWindowOpenHandler(() => ({ action: 'deny' }));
    contents.on('will-attach-webview', (event) => event.preventDefault());
  });
}
