// Тестовая сборка (функция cargo `webdriver`, npm run build:e2e) открывает
// порт отладки WebView2 и не должна попасть к пользователю. Её заголовок окна
// содержит пометку (`WEBDRIVER_BUILD_MARKER` в src-tauri/src/lib.rs); строка
// лежит в исполняемом файле в UTF-8.
export const WEBDRIVER_BUILD_MARKER = 'ТЕСТОВАЯ СБОРКА (webdriver)';

export function isWebdriverBuild(executable: Buffer): boolean {
  return executable.includes(Buffer.from(WEBDRIVER_BUILD_MARKER, 'utf8'));
}
