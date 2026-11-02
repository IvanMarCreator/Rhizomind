// Хозяин правой панели для тестов: состояние реактивно, как раскладка окна
// (`WindowModel.layout`), и записи в хранилище собираются в список.
import { defaultRightPanel, type RightPanel } from '../core/layout/layout.ts';
import type { RightPanelHost } from './panel.svelte.ts';

export interface TestPanelHost extends RightPanelHost {
  /** Все изменения: новое состояние и признак записи. */
  readonly writes: { next: RightPanel; persist: boolean }[];
  /** Только записанные в хранилище состояния. */
  readonly persisted: RightPanel[];
}

export function testPanelHost(initial: Partial<RightPanel> = {}): TestPanelHost {
  let state = $state.raw<RightPanel>({ ...defaultRightPanel(), ...initial });
  const writes: { next: RightPanel; persist: boolean }[] = [];
  return {
    writes,
    get persisted() {
      return writes.filter((w) => w.persist).map((w) => w.next);
    },
    get state() {
      return state;
    },
    update(next, persist) {
      state = next;
      writes.push({ next, persist });
    },
  };
}
