import { app, BrowserWindow, Menu } from 'electron';
import path from 'node:path';
import { APP_ORIGIN, handleAppProtocol, registerAppScheme } from './app-protocol';
import { registerIpc } from './ipc';
import { applyLockdown } from './security/lockdown';

// Собранный код: dist/main/main.js, dist/preload/preload.js, dist/renderer/
const DIST = path.join(__dirname, '..');

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  registerAppScheme();

  app.on('second-instance', () => {
    const win = BrowserWindow.getAllWindows()[0];
    if (win) {
      if (win.isMinimized()) win.restore();
      win.focus();
    }
  });

  app.whenReady().then(() => {
    applyLockdown();
    handleAppProtocol(path.join(DIST, 'renderer'));
    registerIpc();
    if (process.platform !== 'darwin') Menu.setApplicationMenu(null);
    createWindow();

    app.on('activate', () => {
      if (BrowserWindow.getAllWindows().length === 0) createWindow();
    });
  });

  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') app.quit();
  });
}

function createWindow(): void {
  const win = new BrowserWindow({
    width: 1200,
    height: 800,
    minWidth: 720,
    minHeight: 480,
    title: 'Rhizomind',
    show: false,
    webPreferences: {
      preload: path.join(DIST, 'preload', 'preload.js'),
      contextIsolation: true, // интерфейс изолирован от preload и Node.js
      sandbox: true, // процесс интерфейса в песочнице ОС
      nodeIntegration: false,
      nodeIntegrationInSubFrames: false,
      webSecurity: true,
      webviewTag: false,
      spellcheck: false,
      navigateOnDragDrop: false,
      devTools: !app.isPackaged,
    },
  });

  win.once('ready-to-show', () => win.show());
  void win.loadURL(`${APP_ORIGIN}/index.html`);
}
