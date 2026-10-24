import { contextBridge, ipcRenderer } from 'electron';
import { API_VERSION, type RhizomindApi } from '../shared/api';
import { EDITION } from '../shared/edition';
import { IPC } from '../shared/ipc-channels';

/**
 * Мост между интерфейсом и ядром. В window.rhizomind попадает только
 * этот объект: ни ipcRenderer, ни require, ни Node.js интерфейсу не видны.
 */
const api: RhizomindApi = Object.freeze({
  apiVersion: API_VERSION,
  edition: EDITION,
  vault: Object.freeze({
    open: () => ipcRenderer.invoke(IPC.vaultOpen),
    current: () => ipcRenderer.invoke(IPC.vaultCurrent),
    listNotes: () => ipcRenderer.invoke(IPC.vaultListNotes),
    readNote: (path: string) => ipcRenderer.invoke(IPC.vaultReadNote, path),
    writeNote: (path: string, content: string) => ipcRenderer.invoke(IPC.vaultWriteNote, path, content),
    createNote: (title: string) => ipcRenderer.invoke(IPC.vaultCreateNote, title),
  }),
  links: Object.freeze({
    resolve: (target: string) => ipcRenderer.invoke(IPC.linksResolve, target),
    backlinks: (path: string) => ipcRenderer.invoke(IPC.linksBacklinks, path),
  }),
});

contextBridge.exposeInMainWorld('rhizomind', api);
