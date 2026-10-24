import type { RhizomindApi } from './api';

declare global {
  interface Window {
    /** Мост к ядру (src/preload/preload.ts). */
    readonly rhizomind: RhizomindApi;
  }
}

export {};
