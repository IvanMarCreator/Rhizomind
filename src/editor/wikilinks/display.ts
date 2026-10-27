// Отображение вики-ссылки в живом предпросмотре (С3.4, вид 5Б): какие части
// `[[…]]` спрятать и что оформить ссылкой. Текст заметки не меняется.
// - `[[Альфа/Встреча]]`, `[[/Встреча]]` — видно только имя «Встреча»;
// - `[[Встреча#Итоги]]` — «Встреча › Итоги» (`#` заменяет значок « › »);
// - `[[Встреча|встречи]]` — только псевдоним.
// Недопустимая цель (`[[#Заголовок]]`, `..`, `C:\…`, `file:` и т.п. — правила
// parseTarget, те же, что в ядре) — не ссылка: остаётся обычным текстом.
import { parseTarget, type LinkTarget } from '../../core/wikilinks/index.ts';
import type { Piece } from '../live-preview/markdown.ts';

/**
 * Ключ цели для разрешения ядром (`links_resolve`): путь без заголовка и
 * псевдонима, как его разбирает ядро: `Встреча`, `Альфа/Встреча`, `/Встреча`.
 */
export function targetKey(target: LinkTarget): string {
  return `${target.from_root ? '/' : ''}${[...target.folders, target.name].join('/')}`;
}

/** Участок `[from, to)` без пробелов и табуляций по краям. */
function trimmed(text: string, from: number, to: number): [number, number] {
  while (from < to && (text[from] === ' ' || text[from] === '\t')) from++;
  while (to > from && (text[to - 1] === ' ' || text[to - 1] === '\t')) to--;
  return [from, to];
}

/**
 * Части вики-ссылки строки `text`: `[[` в позиции `open`, `]]` — в `close`.
 * `null` — цель недопустима, это не ссылка.
 */
export function wikiPieces(text: string, open: number, close: number): Piece[] | null {
  const innerFrom = open + 2;
  const inner = text.slice(innerFrom, close);
  const target = parseTarget(inner);
  if ('invalid' in target) return null;
  const end = close + 2;
  const link = { from: open, to: end };
  const mark = (from: number, to: number): Piece => ({
    kind: 'mark',
    cls: 'cm-rz-wikilink',
    wiki: inner,
    key: targetKey(target),
    link,
    from,
    to,
  });

  const bar = inner.indexOf('|');
  if (target.alias !== null && bar >= 0) {
    const [from, to] = trimmed(text, innerFrom + bar + 1, close);
    return [{ kind: 'hide', from: open, to: from }, mark(from, to), { kind: 'hide', from: to, to: end }];
  }

  // Цель без псевдонима: имя — последний компонент пути до `#`.
  let targetEnd = bar >= 0 ? innerFrom + bar : close;
  if (bar >= 0 && text[targetEnd - 1] === '\\') targetEnd--;
  const hash = text.indexOf('#', innerFrom);
  const pathEnd = hash >= 0 && hash < targetEnd ? hash : targetEnd;
  const [pathFrom, pathTo] = trimmed(text, innerFrom, pathEnd);
  const nameFrom = text.lastIndexOf('/', pathTo - 1) + 1;
  const visibleFrom = Math.max(nameFrom, pathFrom);
  const nameTo = visibleFrom + target.name.length;

  if (target.heading === null) {
    return [{ kind: 'hide', from: open, to: visibleFrom }, mark(visibleFrom, nameTo), { kind: 'hide', from: nameTo, to: end }];
  }
  const [headingFrom, headingTo] = trimmed(text, pathEnd + 1, targetEnd);
  return [
    { kind: 'hide', from: open, to: visibleFrom },
    mark(visibleFrom, headingTo),
    { kind: 'widget', widget: 'wiki-heading', from: nameTo, to: headingFrom },
    { kind: 'hide', from: headingTo, to: end },
  ];
}
