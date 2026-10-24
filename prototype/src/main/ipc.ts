import { BrowserWindow, dialog, ipcMain, type IpcMainInvokeEvent } from 'electron';
import type { NoteRef } from '../shared/api';
import { IPC } from '../shared/ipc-channels';
import { parseWikiLinks, resolveLinkTarget } from '../core/wikilinks';
import { APP_ORIGIN } from './app-protocol';
import { Vault } from './vault/vault';

let vault: Vault | null = null;

/** Обработчик принимает вызовы только от интерфейса самого приложения. */
function handle(channel: string, fn: (...args: unknown[]) => unknown): void {
  ipcMain.handle(channel, async (event: IpcMainInvokeEvent, ...args: unknown[]) => {
    const origin = event.senderFrame?.url ? new URL(event.senderFrame.url).origin : '';
    if (origin !== APP_ORIGIN) throw new Error('Вызов из недоверенного источника');
    return fn(...args);
  });
}

function requireVault(): Vault {
  if (!vault) throw new Error('Хранилище не открыто');
  return vault;
}

function str(value: unknown, name: string, max = 1024): string {
  if (typeof value !== 'string' || value.length > max) throw new Error(`Некорректный параметр: ${name}`);
  return value;
}

export function registerIpc(): void {
  handle(IPC.vaultOpen, async () => {
    const win = BrowserWindow.getFocusedWindow();
    const options: Electron.OpenDialogOptions = {
      title: 'Открыть хранилище',
      properties: ['openDirectory', 'createDirectory'],
    };
    const result = win ? await dialog.showOpenDialog(win, options) : await dialog.showOpenDialog(options);
    const dir = result.filePaths[0];
    if (result.canceled || !dir) return null;
    vault = await Vault.open(dir);
    return vault.info();
  });

  handle(IPC.vaultCurrent, async () => (vault ? vault.info() : null));
  handle(IPC.vaultListNotes, () => requireVault().listNotes());
  handle(IPC.vaultReadNote, (path) => requireVault().readNote(str(path, 'path')));
  handle(IPC.vaultWriteNote, (path, content) =>
    requireVault().writeNote(str(path, 'path'), str(content, 'content', 10 * 1024 * 1024)),
  );
  handle(IPC.vaultCreateNote, (title) => requireVault().createNote(str(title, 'title', 200)));

  handle(IPC.linksResolve, async (target) => {
    const notes = await requireVault().listNotes();
    return resolveLinkTarget(str(target, 'target'), notes);
  });

  // Простая реализация для v0.1: перечитывает все заметки.
  // Позже заменяется постоянным индексом ссылок.
  handle(IPC.linksBacklinks, async (path) => {
    const current = requireVault();
    const wanted = str(path, 'path');
    const notes = await current.listNotes();
    const result: NoteRef[] = [];
    for (const note of notes) {
      if (note.path === wanted) continue;
      const text = await current.readNote(note.path);
      const linksHere = parseWikiLinks(text).some((l) => resolveLinkTarget(l.target, notes)?.path === wanted);
      if (linksHere) result.push(note);
    }
    return result;
  });
}
