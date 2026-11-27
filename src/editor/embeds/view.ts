import { core, type EmbedOutcome, type EmbedSource } from '../../api/index.ts';
import { t } from '../../i18n/index.ts';
import './view.css';
import { noteBody, type ShowEmbed } from './note.ts';

export const label = (source: EmbedSource) => source.kind === 'path' ? source.path.at(-1) ?? '' : source.target;

// Очередь ограничивает одновременно ожидающие IPC-ответы. Прокрученные
// за окно виджеты снимаются из очереди до запуска чтения.
const pending = new Set<() => Promise<void>>();
let active = 0;
function pump() {
  while (active < 2 && pending.size) {
    const next = pending.values().next().value;
    if (!next) return;
    pending.delete(next);
    active++;
    void next().finally(() => { active--; pump(); });
  }
}

const INLINE_PIXEL_BUDGET = 8 * 1024 * 1024;
let visiblePixels = 0;

export function previewInto(element: HTMLElement, source: EmbedSource, show: ShowEmbed, measure?: () => void) {
  let pixels = 0;
  let alive = true;
  element.contentEditable = 'false';
  element.classList.add('rz-embed');
  element.textContent = t('embed.loading');
  const read = async () => {
    let result: EmbedOutcome;
    try { result = await core.embedRead({ source }); }
    catch { result = { kind: 'unavailable', reason: 'unavailable' }; }
    if (!alive) return;
    element.replaceChildren();
    switch (result.kind) {
      case 'unavailable':
        element.append(label(source) + ' — ' + t(`embed.${result.reason}`));
        break;
      case 'file': {
        const extension = result.name.match(/\.([^.]+)$/u)?.[1]?.toUpperCase();
        element.append(result.name + ' · ' + (extension ?? t('embed.file')));
        break;
      }
      case 'note':
        element.append(noteBody(result.text,result.path,show));
        break;
      case 'image': {
        const count = result.width * result.height;
        if (measure && count + visiblePixels > INLINE_PIXEL_BUDGET) {
          const button = document.createElement('button');
          button.type = 'button';
          button.className = 'rz-embed-link';
          button.textContent = result.name;
          button.onclick = () => show(source, button);
          element.append(button);
          break;
        }
        if (measure) { pixels = count; visiblePixels += count; }
        const image = document.createElement('img');
        image.width = result.width; image.height = result.height;
        image.alt = result.name;
        image.onload = () => { if (alive) measure?.(); };
        // Только новый PNG из пикселей ядра. Пути и исходные URL сюда не поступают.
        image.src = 'data:image/png;base64,' + result.png;
        element.append(image);
        break;
      }
    }
    measure?.();
  };
  pending.add(read); pump();
  return () => { alive = false; pending.delete(read); visiblePixels -= pixels; pixels = 0; };
}
