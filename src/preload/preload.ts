import { contextBridge } from 'electron';
import { API_VERSION, type RhizomindApi } from '../shared/api';

// Мост между интерфейсом и ядром. Интерфейсу виден только этот объект:
// ни ipcRenderer, ни require, ни Node.js в window не попадают.
const api: RhizomindApi = Object.freeze({ apiVersion: API_VERSION });

contextBridge.exposeInMainWorld('rhizomind', api);
