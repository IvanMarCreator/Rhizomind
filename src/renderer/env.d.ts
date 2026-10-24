import type { RhizomindApi } from '../shared/api';

declare global {
  interface Window {
    readonly rhizomind: RhizomindApi;
  }
}

export {};
