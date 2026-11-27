import { Text } from '@codemirror/state';
import type { EmbedSource, VaultPath } from '../../api/index.ts';
import { t } from '../../i18n/index.ts';
import { parseTarget } from '../../core/wikilinks/index.ts';
import { fenceStep, findProperties, parseLine, type OpenFence, type Piece } from '../live-preview/markdown.ts';

export type ShowEmbed = (source: EmbedSource, origin: HTMLElement) => void;
const LINE_LIMIT = 200;

function compact(target: string, from: VaultPath, show: ShowEmbed): HTMLButtonElement {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'rz-embed-link';
  const parsed = parseTarget(target);
  button.textContent = 'invalid' in parsed ? target || '[[]]' : parsed.alias ?? parsed.name;
  button.title = target;
  button.onclick = () => show({ kind: 'target', from, target }, button);
  return button;
}

// Применяем те же диапазоны, что у живого редактора, только к текстовым узлам.
// Переход по вложенной ссылке — отдельное действие. Автоматических чтений здесь нет.
function formatLine(node: HTMLElement, text: string, previous: string | null, from: VaultPath, show: ShowEmbed) {
  const line = parseLine(text, previous);
  if (line.lineClass) node.className += ' ' + line.lineClass;
  const starts = new Map<number, Piece[]>(), ends = new Map<number, Piece[]>();
  for (const piece of line.pieces) {
    if (piece.from >= piece.to) continue;
    const begin = starts.get(piece.from) ?? [], end = ends.get(piece.to) ?? [];
    begin.push(piece); end.push(piece);
    starts.set(piece.from, begin); ends.set(piece.to, end);
  }
  const positions = [...new Set([0, text.length, ...starts.keys(), ...ends.keys()])].sort((a,b) => a-b);
  const active = new Map<string, number>();
  let hidden = 0;
  for (const [i, pos] of positions.entries()) {
    for (const p of ends.get(pos) ?? []) {
      if (p.kind === 'mark') {
        const left = (active.get(p.cls) ?? 1) - 1;
        if (left) active.set(p.cls,left); else active.delete(p.cls);
      } else hidden--;
    }
    for (const p of starts.get(pos) ?? []) {
      if (p.kind === 'mark') active.set(p.cls, (active.get(p.cls) ?? 0) + 1);
      else {
        hidden++;
        if (p.kind === 'embed') node.append(compact(p.target,from,show));
        if (p.kind === 'widget') node.append(({ bullet: '•', checkbox: '☐', 'checkbox-checked': '☑', rule: '—', 'wiki-heading': ' › ' })[p.widget]);
      }
    }
    const next = positions[i+1];
    if (hidden || next === undefined || next <= pos) continue;
    const segment = text.slice(pos,next);
    if (!active.size) node.append(segment);
    else {
      const span = document.createElement('span');
      span.className = [...active.keys()].join(' ');
      span.textContent = segment;
      node.append(span);
    }
  }
}

export function noteBody(text: string, path: VaultPath, show: ShowEmbed): HTMLElement {
  const body = document.createElement('span');
  body.className = 'rz-embed-note';
  if (!text) { body.textContent = t('embed.empty'); return body; }
  const lines = text.replace(/^\uFEFF/u,'').split(/\r?\n/);
  const doc = Text.of(lines);
  const props = findProperties(doc);
  let open: OpenFence | null = null;
  for (let i = 1; i <= Math.min(LINE_LIMIT, doc.lines); i++) {
    const line = doc.line(i);
    const row = document.createElement('span');
    row.className = 'rz-embed-line';
    const wasOpen = open;
    if (props && line.from <= props.to) row.textContent = line.text;
    else {
      open = fenceStep(open, line.text, line.from, line.to, []);
      if (wasOpen || open) { row.classList.add('cm-rz-code'); row.textContent = line.text; }
      else formatLine(row,line.text,lines[i-2] ?? null,path,show);
    }
    body.append(row);
  }
  if (doc.lines > LINE_LIMIT) body.append(t('embed.more', { count: LINE_LIMIT }));
  return body;
}
