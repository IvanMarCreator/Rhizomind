// Разрешена ли вики-ссылка (С3.4: неразрешённая выглядит иначе). Решает ядро
// (`links_resolve`); редактор хранит ответы по ключу цели (`targetKey`) и
// спрашивает только о ссылках видимой части (live-preview/index.ts).
import { StateEffect, StateField } from '@codemirror/state';

/** Ответы ядра: ключ цели → разрешена ли. */
export const resolvedEffect = StateEffect.define<ReadonlyMap<string, boolean>>();

export interface Resolution {
  /** Известные ответы; ключа нет — ещё не спрашивали (ссылка выглядит разрешённой). */
  known: ReadonlyMap<string, boolean>;
  /** Растёт при изменении дерева хранилища: ответы надо спросить заново. */
  generation: number;
}

/** Дерево хранилища изменилось: прежние ответы остаются до новых (без мигания), но спрашиваются снова. */
export const staleEffect = StateEffect.define<null>();

export const resolutionField = StateField.define<Resolution>({
  create: () => ({ known: new Map(), generation: 0 }),
  update(value, tr) {
    let next = value;
    for (const effect of tr.effects) {
      if (effect.is(resolvedEffect)) {
        const known = new Map(next.known);
        for (const [key, resolved] of effect.value) known.set(key, resolved);
        next = { known, generation: next.generation };
      } else if (effect.is(staleEffect)) {
        next = { known: next.known, generation: next.generation + 1 };
      }
    }
    return next;
  },
});

/** Ссылка неразрешена по последнему ответу ядра. */
export function isUnresolved(resolution: Resolution | undefined, key: string): boolean {
  return resolution?.known.get(key) === false;
}
