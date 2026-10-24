import { app, BrowserWindow, Menu } from 'electron';
import path from 'node:path';
import { APP_ORIGIN } from './app-asset-path';
import { handleAppProtocol, registerAppScheme } from './app-protocol';

// Собранный код: dist/main/main.js, dist/preload/preload.cjs, dist/renderer/.
const DIST = path.join(import.meta.dirname, '..');

registerAppScheme();

function createWindow(): void {
  const win = new BrowserWindow({
    width: 1200,
    height: 800,
    minWidth: 720,
    minHeight: 480,
    title: 'Rhizomind',
    show: false,
    webPreferences: {
      preload: path.join(DIST, 'preload', 'preload.cjs'),
      contextIsolation: true, // интерфейс изолирован от моста и Node.js
      sandbox: true, // процесс интерфейса в песочнице ОС
      nodeIntegration: false,
      nodeIntegrationInSubFrames: false,
      webSecurity: true,
      webviewTag: false,
      devTools: !app.isPackaged,
    },
  });
  win.once('ready-to-show', () => win.show());
  void win.loadURL(`${APP_ORIGIN}/`);
}

void app.whenReady().then(() => {
  handleAppProtocol(path.join(DIST, 'renderer'));
  if (process.platform !== 'darwin') Menu.setApplicationMenu(null);
  createWindow();
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});
